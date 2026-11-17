# Bitácora: el instalador y el pendrive en vivo (instalador)

Rama `instalador`, sacada de `x86` (cade3fea: la imagen de usar de x86). Diseño: S32 (con lo decidido el 2026-11-16:
el instalador es DashOS de verdad arrancado de un pendrive, usable en vivo; y la primera versión encoge Windows), S14,
S0, S6b. Papel: implementa. Sala limpia: no se leyó código de otros sistemas; las especificaciones (UEFI 2.10, GPT,
FAT de Microsoft, NTFS por su documentación pública) sí.

## 2026-11-16 y 17

### 0. Unión de `revision` (df1a3ca9)

Tres conflictos, resueltos con las dos partes:

- `usuario/src/lib.rs`: `pub mod cuentas` (revision) y `#[cfg(aarch64)] mod copiar` (x86); las funciones X11 de la
  llamada 60 (x86) y las de S35 de la 48 (`maquina_prestar_registros`, `maquina_dma_identidad`, revision).
- `kernel/src/llamadas.rs`: la tabla de la cabecera con las dos líneas. Y algo que git no vio: las operaciones 40 y 41
  de la llamada 48 (prestar registros y DMA a un dominio, S35) usan métodos de la máquina de ARM; en x86 no existen
  (allí los aparatos pasan con la llamada 60 y la IOMMU). Van con `#[cfg(target_arch = "aarch64")]`; en x86 dan
  `E_ARGUMENTO` como cualquier operación desconocida.
- `tools/disco_archivos.py`: la comprobación de las cuentas (revision) va dentro del `else` de x86 básico, como la del
  informe: el x86 sin escritorio no las escribe.
- Números: `revision` no añade llamadas ni tipos de llave nuevos (solo las operaciones 40 y 41 de la 48), así que
  `puertos_permitir` sigue en la 59 y la llave de puertos en el 13, como dice `x86-uso.md`.

### Encoger Windows: el dominio de x86 (estudio, 2026-11-16)

Decisión del usuario: la primera versión encoge NTFS, con `ntfsresize` dentro de un dominio Linux (S35). En x86 ese
dominio no existe. Lo que hay y lo que falta:

- **Hay** (X10, X11): el hipervisor de x86 en el kernel (`base/x86_64/hiper.rs`, AMD-V con NPT y VT-x con EPT, hasta
  8 vCPU), que deja al servicio cada salida ya interpretada (puertos, MMIO con la instrucción decodificada, HLT, MSR,
  VMCALL), el temporizador del plazo del TSC y el arranque en modo protegido de 32 bits con ESI (el protocolo de 32 bits
  de Linux). Y de ARM, sin `unsafe` y genérico: virtio-mmio, las colas, virtio-blk (`caja-linux/vmm`), la consola de
  virtio y el disco por tramos del servicio "disco" (`DiscoNvme` de `caja_linux.rs`).
- **Falta**, todo en el lado del servicio (el kernel no necesita nada): cargar un bzImage (`boot_params`, e820,
  cmdline, initrd; ~250 líneas), una tabla MP mínima (~120), el APIC local (~350) y el IOAPIC (~180) emulados, puertos
  de relleno (PIC, PIT, CMOS, i8042; ~80), la consola (virtio-console y un 8250 mínimo para `earlycon`; ~200), el
  disco (virtio-mmio-blk, sacando `DiscoNvme` a un módulo común y quitando el GIC de la máquina; ~100), el bucle del
  monitor de x86 (~450) y el Linux con su initramfs (tinyconfig de 64 bits sin PCI ni ACPI, `ntfsresize` estático y un
  `/init` que lo ejecuta y da el resultado; ~200 de configuración y C).
- **Tamaño**: unas 1.700 líneas nuevas, más ~1.300 de virtio que se reutilizan, y la compilación de Linux y ntfs-3g.
  **Riesgo principal**: sería el primer Linux en el hipervisor de x86, sin firmware: relojes (sin ACPI ni PIT, la
  frecuencia del TSC por CPUID 0x15 o `tsc_early_khz=`, que el servicio aún no sabe) e interrupciones (el IOAPIC y su
  EOI). Hasta que salga `earlycon`, a ciegas.
- **Decisión**: es grande. Como pidió el usuario, primero todo lo demás del instalador; el encogido queda detrás de esa
  pieza, con el plan, las comprobaciones previas (hibernado, sucio, BitLocker, casi lleno), la copia de las tablas y
  la prueba hostil hechos, y el paso de `ntfsresize` como lo único que falta.

### 1. La biblioteca del instalador (`usuario/src/instalador/`, probada en el PC)

Sin nada del sistema (solo `alloc`): la usa el servicio en DashOS y `instalar-pc` en el PC, con las mismas líneas.

- `disco.rs`: el rasgo `Disco` (sectores, leer, escribir, vaciar), leer y escribir bytes sueltos, el CRC-32, los GUID.
- `gpt.rs`: el MBR y la GPT con **sus dos copias**. Se escribe solo si las dos están buenas e iguales. Rota la del final,
  rota la del principio, distintas, MBR híbrido, particiones que se pisan o se salen: se lee, se dice en llano y no se
  escribe. Los tipos conocidos (ESP, Windows, recuperación, Linux, swap, LUKS, LVM, RAID, APFS, HFS+, BIOS de GRUB) con
  su nombre en llano. Escribir: la copia del final primero, luego la lista del principio, luego su cabecera, vaciando
  entre medias. Un corte en cualquier sector deja la tabla de antes, la nueva, o una "instalación de DashOS cortada"
  que se reconoce (las copias solo difieren en la entrada de DashOS, o una cabecera buena con su lista a medias que
  casa con la otra copia) y deja repetir; la prueba corta en cada sector.
- `reconocer.rs`: por la firma, no por el tipo: NTFS, BitLocker (`-FVE-FS-`), exFAT, FAT12/16/32, ext2/3/4 (limpio o
  no), btrfs, swap, LUKS, LVM, APFS, HFS+, DashFS. De NTFS, leyendo su MFT (registros con su corrección de secuencia,
  tramos de datos): la etiqueta y la bandera de sucio de `$Volume`, los clusters usados de `$Bitmap`, y si
  `hiberfil.sys` (buscado en los índices del cajón raíz) empieza por "hibr" (hibernado o inicio rápido). Probado contra
  el Windows 11 del banco: 3.385.930 clusters usados de 12.767.487 (13,9 GB, lo mismo que `ntfsresize --info`), limpio,
  no hibernado; y tras encogerlo con `ntfsresize`, sucio (`ntfsinfo` dice lo mismo: 0x0201).
- `plan.rs`: el plan. Modos: hueco (el más grande), disco entero, reinstalar (sobre la partición de DashOS que hay) y
  encoger (NTFS). La ESP: la del disco si tiene una (FAT16/32), si no una nueva de 260 MB. Lo que para, en llano: es el
  pendrive del que arrancó, MBR híbrido o antiguo, tabla que no se puede escribir, sin hueco ("el hueco libre más grande
  es de X y DashOS necesita Y"), dos ESP, y para encoger: BitLocker, hibernado o inicio rápido, comprobación pendiente,
  casi lleno (con un margen para Windows de 10 GB). El texto lleva el dibujo antes y después (una fila de 64 columnas,
  con letra por partición y su leyenda), lo que se hace y se pierde, los avisos, y debajo las líneas del plan; `leer`
  desconfía (líneas repetidas, que sobran, versión, incoherencias) y `comprobar` vuelve a calcularlo contra el disco tal
  como está: la huella de la tabla tiene que ser la misma, y un plan escrito a mano que pise algo no pasa.
- `fat.rs`: FAT16 y FAT32 (FAT12 no se escribe): leer cajones (con nombres largos), crear cajones y ficheros con su
  nombre largo y su corto (`BASE~N`), sustituir sin escribir encima, y formatear una FAT32 nueva. Orden: datos, cadena
  en las dos FAT, entrada del cajón (un sector), soltar lo de antes. Cortado en cada sector, el cargador de Windows sigue
  entero y la FAT se abre; `fsck.fat -n` no ve nada en lo que deja el instalador.
- `nvram.rs`: `EFI_LOAD_OPTION` con la ruta corta (nodo de disco duro con el GUID de la ESP y el fichero
  `\EFI\DashOS\DASHOS.EFI`), leerla, `BootOrder`, y decidir: si ya hay una entrada de DashOS a esa ESP se reusa su
  número; si no, el primero libre; se añade **al final** de `BootOrder` y se pone `BootNext`.
- `aplicar.rs`: el orden de la instalación, pensado para un corte de luz en cualquier sitio: volver a leer y comprobar
  el plan; copia de las tablas (las dos puntas del disco y, si se encoge, el primer MiB de esa partición); encoger (si
  toca); el sistema al hueco y crecido (aún sin entrada en la tabla: cortado, el hueco sigue libre); la ESP; la tabla;
  la NVRAM. La prueba lo corta en **cada sector** de la instalación entera junto a un Windows falso: Windows (sus
  particiones y su cargador) queda intacto siempre, y la instalación se repite (en hueco o reinstalando) y acaba.
- 15 pruebas, en `make prueba` (18 s). `pc.rs` (`instalar-pc ver|plan`): lo mismo contra un disco de verdad, para el
  banco.

### 2. El pendrive en vivo (x86)

- **Cargador** (`uefi/src/x86_64/vivo.rs`): por `EFI_LOADED_IMAGE_PROTOCOL` sabe desde qué partición arrancó (el GUID
  del nodo de disco duro de su ruta) y cuál es su propio fichero. Si en esa partición está `\EFI\DashOS\VIVO.IMG` (el
  sistema: un DashFS entero), lo lee entero a memoria de datos del cargador, junto con su propio fichero (lo que se
  instala) y, si está, `\EFI\DashOS\SINMANOS.TXT`. `InfoArranque` pasa a la versión 2: la tabla de servicios de
  ejecución de UEFI, la zona en vivo y el GUID de la partición de arranque (el kernel acepta la 1 y la 2).
- **Kernel** (`inicio_x86.rs`): si hay zona en vivo, dos objetos de memoria ("vivo", el sistema, para leer y escribir;
  "vivo-original", la zona entera, de solo lectura) y el permiso de variables. Y en vivo el canal "disco" lo sirve
  `disco-vivo`; el NVMe pasa a "nvme" y el AHCI a "sata", que solo usa el instalador.
- **`disco-vivo`** (servicio nuevo): el sistema en memoria, servido por el canal "disco" con el protocolo del NVMe (sin
  anillo: el servicio de archivos ya sabe ir por el canal). Clientes: `archivos` (leer y escribir) y `comprobador` (leer).
  Lo que se cambia queda en la memoria y se pierde al apagar; **los discos del PC no se tocan**: `archivos` ni los ve.
- **`make -C kernel pendrive-x86`** (`tools/pendrive_x86.py`): `target/pendrive-x86.img`, GPT con una ESP FAT32 (64 MB)
  con `EFI/BOOT/BOOTX64.EFI` (el cargador con la imagen de usar dentro, 5,6 MB), `EFI/DashOS/VIVO.IMG` (el DashFS del
  disco de serie, 7 MB) y `LEEME.TXT`. Se graba con `dd`. `make -C kernel vivo-x86` lo arranca en QEMU como USB con un
  NVMe vacío al lado.
- Medido: del firmware al escritorio de entrada, unos 4 s con KVM; el sistema en vivo ocupa 7 MB de RAM (más su copia
  intacta para instalar, otros 7).

### 3. El instalador (servicio)

- `instalador` (servicio, solo en vivo: `inicio` no lo lanza sin "vivo-original"). Los discos enteros por el nombre
  "instalador" en el NVMe y el AHCI (`nvme.rs`, `disco_servidor.rs`: el tramo es el disco entero, aunque su tabla no
  valga, para poder leerla y decir qué le pasa). Usa la biblioteca; pone lo suyo: el sistema y el cargador del pendrive,
  crecer el DashFS (el crate `dashfs`, `FileSystem::grow`), la NVRAM y el registro. El registro va a la consola, a quien
  pregunta y a `Estado:Registros/instalacion` del disco nuevo (de `sistema`, 0644). La copia de las tablas, en
  `EFI/DashOS/TABLAS.BIN` de la ESP.
- **La NVRAM** (decisión, ver abajo): llamada nueva **64** `firmware_variable` (solo x86), con un permiso nuevo
  (`PermisoVariables`, tipo de llave de arranque **20**) que solo tiene el instalador. Leer, la siguiente, y escribir
  solo `Boot####`, `BootOrder` y `BootNext` del GUID global. El kernel llama a los servicios de ejecución del firmware
  (`base/x86_64/uefi.rs`) sin `SetVirtualAddressMap`: con un PML4 aparte (la mitad alta del kernel y, 1:1, solo las
  zonas que el mapa de UEFI marca de ejecución), interrupciones cerradas, la FPU guardada, una pila propia de 64 KB y la
  TLB vaciada al volver. En OVMF: 6 zonas proyectadas; `Boot0005`, `BootOrder` y `BootNext` escritos y releídos.
- Pila de 1 MB (como `archivos`) y 128 MB de tope (copias del sistema y del cargador y el DashFS abierto): con 64 KB se
  caía al abrir el DashFS.
- El canal "instalador": ver, plan, instalar, registro (textos en una memoria). Sin manos: `SINMANOS.TXT` del pendrive
  (un plan o un pedido) se aplica al arrancar.

### 4. La ventana y la orden del Shell, con la confirmación de administrador

- **La ventana "Instalar DashOS"** (`usuario/src/bin/instalar.rs`, con el kit): solo en vivo, la lanza `inicio`. Los
  discos y lo que tienen; un botón por modo ("Usar el espacio libre de nvme0", "Borrar nvme0 entero") que pide el plan y
  lo enseña con su dibujo; "Instalar" (activo solo con un plan en pantalla). No sale encima de la pantalla de entrada:
  se ve al entrar.
- **La orden `Instalar`** (`ordenes/src/base/instalar.rs`): `Instalar` (los discos), `Instalar nvme0 hueco` (el plan; se
  guarda con `>RAM:plan`), `Instalar PLAN RAM:plan` (aplicarlo). La hace el Shell, que tiene la llave del instalador
  sin preguntar (`sesiones`, `PERMISOS`: ver y hacer planes no escribe nada).
- **Una sola confirmación, de administrador** (S6b, S36): la acción nueva **`instalar <número>`** de la tanda
  (`tanda.rs`; el número es el CRC-32 del texto del plan). `guardian` la enseña ("Instalar DashOS en el disco, con el plan
  N (lo que dice el plan se escribe; lo de otros sistemas no se toca)") y pide la contraseña; `sesiones` se lo dice al
  instalador por su llave sin marca (la recibe de `inicio`, `A_SESIONES`), y el instalador solo aplica ese plan, una vez
  y en cinco minutos. Sin confirmación, "sin la confirmación de un administrador para este plan".
- `tools/prueba_instalador_ventana.py`: como una persona (teclado y tableta por QMP): entra Ana, "Borrar nvme0 entero",
  el plan en la ventana, "Instalar", la ventana del sistema pide la contraseña, se instala; en el Shell, `Instalar` e
  `Instalar nvme0 reinstalar`; y DashOS arranca del disco. 8 de 8 (capturas `ventana-NN-*.png`).

### 5. El banco de AmiCachy (`tools/banco_instalador.py`, `tools/prueba_instalador.py`)

- Los discos base no se tocan: cada prueba en una capa qcow2 propia en `~/recuperar/geosian-7d/instalador-vm/`, con sus
  OVMF_VARS; el guion se niega a escribir fuera de ahí y comprueba al empezar y al acabar el tamaño y la fecha de los
  base (y la huella del vacío). Mirar una capa sin root: `qemu-storage-daemon` con exportación FUSE (de solo lectura,
  salvo para preparar las hostiles).
- Capas de partida: `win-hueco-base` (Windows 11 con un hueco de 21 GB: su NTFS encogido con `ntfsresize` desde fuera y
  su entrada de la GPT recortada, solo en la capa) y `deb-hueco-base` (Debian 13 con un hueco de 29 GB, `resize2fs`).
  Este Windows 11 deja el NTFS sucio tras encogerlo (banderas 0x0201) aunque arranque y se apague bien dos veces: sirve
  de hostil "NTFS sucio".
- **Resultado** (16 + 11 de 27 bien):
  - vacío: disco entero; GPT con sus dos copias iguales; DashOS arranca por la NVRAM (el próximo arranque) y eligiéndolo
    en el menú del firmware (OCR de la captura del menú de OVMF).
  - Windows 11 con hueco: instalado; sus cuatro particiones iguales **byte a byte** y en la ESP solo `EFI/DashOS`
    nuevo (los ficheros de Windows, iguales); DashOS arranca (NVRAM y menú: "Windows Boot Manager" primero, "DashOS"
    después); Windows llega a su pantalla de bloqueo y se apaga limpio (capturas `windows-windows-NN.png`).
  - Debian 13 con hueco: lo mismo; Debian llega a `login:` por el puerto serie y se apaga limpio.
  - Hostiles: el pendrive como destino (el mismo pendrive puesto también como disco SATA: "es el pendrive del que
    arrancó DashOS, no se ofrece"); las dos copias de la GPT buenas pero distintas (no se escribe); sin hueco (el
    Windows de serie, disco lleno: "no hay sitio: el hueco libre más grande es de 1 MB y DashOS necesita 4,0 GB");
    encoger Windows (se para antes de tocar nada: falta el dominio) y Windows intacto después; NTFS sucio, hibernado
    (un `hiberfil.sys` con "HIBR" puesto con `ntfscp`) y BitLocker (la firma `-FVE-FS-`): se dice y no se toca; corte de
    luz (QEMU muerto con `kill -9`) al copiar el sistema y tras poner el cargador: Windows intacto byte a byte, y la
    instalación se repite y acaba.
- **Lo que enseñó el banco**: una copia de la GPT simplemente rota no sirve de hostil en QEMU, porque el firmware (EDK2)
  la rehace desde la otra al arrancar; la prueba usa dos copias buenas y distintas. Y un `BootNext` que dejó una
  instalación cortada manda el siguiente arranque al disco, no al pendrive: para repetir, la prueba arranca con una
  NVRAM limpia (como quien elige el pendrive en el menú del firmware).

### Decisiones tomadas aquí (pendientes del usuario)

1. **NVRAM desde el sistema, no desde el cargador**: la entrada se escribe en el momento con los servicios de ejecución
   del firmware (como hace `efibootmgr`), con un permiso que solo tiene el instalador y solo para las variables de
   arranque. Alternativa descartada: que el cargador del pendrive la escriba en el siguiente arranque (falla si se quita
   el pendrive antes). Riesgo: firmwares con fallos en sus servicios de ejecución sin `SetVirtualAddressMap`; probado
   solo en OVMF.
2. **Orden de arranque**: DashOS se añade **al final** de `BootOrder` (el sistema que arrancaba sigue arrancando) y con
   `BootNext` (el primer reinicio entra en DashOS, una vez). Elegir cuál arranca por omisión: en el plan, más adelante.
3. **Tamaños**: DashOS necesita 4 GB como mínimo; la ESP nueva, 260 MB; a Windows se le deja un margen de 10 GB sobre lo
   que usa al encogerlo.
4. **El pendrive en vivo arranca en la pantalla de entrada** con las cuentas de la imagen de pruebas (Ana, Carla): una
   imagen limpia para repartir, con el asistente de primer arranque de `cuentas-2`, cuando esté.
5. **Sin manos**: `EFI/DashOS/SINMANOS.TXT` del pendrive (un plan, o un pedido) se aplica sin preguntar: quien hizo el
   pendrive lo pidió para ese disco (un plan lleva la huella del disco). Es para el banco; un pendrive de serie no lo lleva.
6. **La ventana del instalador** sale sola en vivo; la orden la hace el Shell con la llave del instalador sin preguntar.
7. **Disco MBR**: se lee y se enseña, pero solo se instala en él borrándolo entero (a GPT).
8. La copia de las tablas va a la ESP (`EFI/DashOS/TABLAS.BIN`) y al registro, no al pendrive: DashOS aún no escribe
   en memorias USB en x86.

### Pendiente

- **Encoger NTFS**: el dominio Linux de x86 (unas 1.700 líneas, ver arriba). El plan, las comprobaciones previas, la
  copia del principio de la partición y su hostil ya están; falta lo que ejecuta `ntfsresize`.
- El pendrive de la **Pi 5** (`make tarjeta-pi5` hace la tarjeta; falta su versión en vivo con instalador).
- Una imagen limpia (sin las cuentas de prueba ni el daño de A10) y el asistente de primer arranque.
- Que `guardian` enseñe el resumen del plan (disco y modo) y no solo su número.
- Windows y Debian en dos discos de la misma máquina (escenario "Windows + Debian"): no montado.
- `Instalar PLAN` desde el Shell de principio a fin (la ventana sí se probó entera; la orden, ver y planear).
