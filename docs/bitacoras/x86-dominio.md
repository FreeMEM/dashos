# Bitácora: el dominio Linux de x86 (x86-dominio)

Rama `x86-dominio`. DashOS arranca en su propio hipervisor de x86-64 una máquina pequeña con Linux para ejecutar
`ntfsresize` sobre una partición NTFS que le presta. Aquí, el lado invitado; el monitor va aparte.

## 2026-11-17: lado invitado

### Qué se hizo

- `caja-linux/dominios/x86_ntfs.config`: el kernel (Linux 7.3.4 de kernel.org, el de medir-linux), `make
  tinyconfig` más este fichero y nada más.
- `caja-linux/dominios/ntfs_dominio.c`: el `/init`, en C sin biblioteca (llamadas al sistema directas). Monta
  devtmpfs y proc, lee la orden de `/proc/cmdline` y ejecuta `/bin/ntfsresize` sobre `/dev/vda`, nunca con `--force`.
  Cada línea que escribe ntfsresize va a `/dev/kmsg` como `dominio: <línea>`. Al final escribe `dominio: memoria ...`
  y `dominio: fin <paso> <código>`, hace sync y apaga. Sin ACPI el kernel no sabe apagar: dice "System halted" y se
  queda en CLI + HLT, con el LAPIC desactivado y los pines del IOAPIC enmascarados.
  - `dashos.ntfs=info`: `ntfsresize --info --no-progress-bar /dev/vda`.
  - `dashos.ntfs=encoger:<bytes>`: primero en seco (`-n -s <bytes>`); si da 0, de verdad (`-s <bytes>`, con "y" por
    la entrada estándar). En info y en seco la entrada es `/dev/null`.
  - Paso `orden` con código 2 si falta la orden o no se entiende; 127 si no se puede ejecutar ntfsresize; 128 + señal
    si muere.
- `tools/dominio_x86.py`: `preparar` (kernel, musl, ntfsresize, `/init`, initramfs), `qemu`, `ver`, `minimo`,
  `grande` y `accesos`. Lo compilado y lo bajado queda en `~/recuperar/geosian-7d/dominio-x86/` (`bzImage`,
  `initramfs.cpio`).

### La línea del kernel

```
earlyprintk=serial,0x3f8,115200,keep tsc_early_khz=<kHz del TSC> tsc=reliable printk.devkmsg=on virtio_mmio.device=4K@0xd0000000:5 dashos.ntfs=<orden>
```

- `earlyprintk=serial,0x3f8,115200,keep`: la consola temprana, sondeada (escribe en el THR cuando el LSR dice
  "vacío"). No hace falta el controlador 8250 ni terminales; `keep` la mantiene hasta el final.
- `tsc_early_khz`: la frecuencia del TSC; el kernel no la calibra y el retardo (`lpj`) sale de ella.
- `tsc=reliable`: que no vigile el TSC con otro reloj (no lo hay) ni mire cada rato el MSR TSC_ADJUST.
- `printk.devkmsg=on`: sin esto, lo que el `/init` escribe en `/dev/kmsg` se recorta a 10 líneas cada 5 s.
- No hacen falta `lpj=`, `no_timer_check` ni `nolapic_timer`.
- Sin `quiet`: el registro entero sale por el 0x3F8 (unos 8,5 KB: 8.500 escrituras al THR y otras tantas lecturas del
  LSR). Con `quiet` se callarían también las líneas del `/init` (nivel 5).

### Opciones del kernel que importan

- Sin ACPI ni PCI. La tabla MP la lee `X86_MPPARSE`, que sin ACPI no se puede quitar.
- Con LAPIC e IOAPIC; sin x2APIC.
- Sin SMP, con `NR_CPUS=1`.
- `VIRTIO_MMIO` y `VIRTIO_MMIO_CMDLINE_DEVICES` con `VIRTIO_BLK`.
- Ningún lector de tablas de particiones: el sector de arranque de NTFS lleva 0x55AA y un lector de MBR podría
  tomarlo por una tabla.
- `FILE_LOCKING`: ntfs-3g pone un cerrojo con `fcntl` al abrir el disco. Sin él, tinyconfig contesta EACCES y
  ntfsresize dice "Permission denied".
- `PHYSICAL_START=0x200000` (de serie, 16 MB): baja la memoria mínima.
- Fuera DMI, MTRR/PAT, MCE, IBT, pila sombra, llaves de protección y mitigaciones: menos MSR que atender.
- `HZ=100` y `NO_HZ_IDLE`: menos salidas por el reloj.
- Sin módulos y con el initramfs sin comprimir. La bzImage va comprimida con XZ, como pone tinyconfig.

### ntfsresize

- ntfs-3g 2026.9.28: `ntfs-3g_ntfsprogs-2026.9.28.tgz`, bajado de download.tuxera.com. Su SHA-256 es
  `350d9415c59f7c3fa74e23985ad2d56423c6a168337368f672e2e362d8f295c8`, la misma que publica Tuxera en la página de la
  versión (GitHub, tuxera/ntfs-3g).
- Enlazado estático con **musl 1.2.6**, compilada aquí para x86-64 de base:
  - SHA-256 `d585fd3b613c66151fc3249e8ed44f77020cb5e6c1e635a616d3f9f82460512a`.
  - Firma GPG buena, de la llave de musl (huella `8364 8929 0BB6 B70F 99FF DA05 56BC DB59 3020 450F`).
  - Cabeceras del kernel sacadas del mismo Linux 7.3.4 (`headers_install`).
- No con la glibc del PC: es la de CachyOS para x86-64-v4. Un estático con ella pide AVX-512 y no arranca en una CPU
  que no lo tenga.
- El aviso de seguridad de musl (CVE de `regcomp` hasta la 1.2.6) no toca: ntfsresize no enlaza `regcomp`.
- Solo `ntfsresize`, con `--disable-ntfs-3g --disable-plugins --without-uuid --without-hd --disable-crypto`.

### Tamaños

| | bytes |
|---|---|
| bzImage | 930.816 |
| initramfs.cpio (sin comprimir) | 356.468 |
| `/init` | 13.080 |
| ntfsresize | 342.672 |

### Las pruebas

QEMU 11.1.3, `-M microvm,acpi=off,pic=off,pit=off,rtc=off,isa-serial=on,ioapic2=off`, con
`-global virtio-mmio.force-legacy=false` (virtio-mmio versión 2). Una CPU con `-cpu host,-x2apic` y KVM, o
`-cpu max,-x2apic` con TCG. Disco: un NTFS de 2 GB hecho con `mkntfs -F -f`, sin tabla, con 311 MB de ficheros puestos
con `ntfscp`.

Dos arranques:

- **qboot**: el firmware de microvm. Entra por el código de 16 bits del kernel, con su tabla MP en la EBDA y su e820.
- **contrato** (`... contrato`): una ROM de 64 KB hecha por la herramienta.
  - En 0xF0000, la tabla MP del contrato: 1 CPU, bus ISA, IOAPIC 1 en 0xFEC00000, ISA 0-15 a los pines 0-15.
  - Desde el vector de reinicio pasa a modo protegido plano y salta a 0x100000 con ESI = boot_params.
  - La zero page (con la e820 exacta del contrato), la línea, la parte protegida de la bzImage y el initrd los pone
    QEMU en la RAM con `-device loader`.
  - Es el arranque de 32 bits que hará DashOS, y funciona.

En QEMU solo cambia el aparato virtio: microvm lo pone en 0xfeb00e00 (512 bytes) con la IRQ 12, y la línea dice
`virtio_mmio.device=512@0xfeb00e00:12`.

Resultados (KVM y TCG, con los dos arranques):

- **info**: "Space in use: 338 MB (15.7%)" y "You might resize at 337420288 bytes"; `fin info 0`.
- **encoger:1073741824**: la prueba en seco y la de verdad acaban bien (`fin encoger 0`). En el PC, `ntfsresize
  --info` da un volumen de 1.073.738.240 bytes. `ntfsfix -n` no ve problemas con la imagen recortada a 1 GiB.
  - Sin recortar, `ntfsfix -n` falla en "alternate boot sector". ntfsresize pone la copia del sector de arranque en
    el último sector del tamaño pedido, así que **DashOS tiene que encoger la partición exactamente a `<bytes>`**.
  - Tras encoger, el volumen queda marcado para chkdsk. Un segundo `info` sin pasar por Windows se niega ("Volume is
    scheduled for check"); en el PC hace falta `-f` para mirarlo.
- **Sucio** (bandera VOLUME_IS_DIRTY puesta en $Volume y en su espejo): `info` y la prueba en seco se niegan ("Volume
  is scheduled for check. Run chkdsk /f..."): `fin info 1` y `fin prueba 1`. El volumen sigue de 2 GB. Tras
  `ntfsfix -d` en el PC, `info` vuelve a dar 0.
- **Volúmenes grandes** (dispersos, casi vacíos, 128 MB): 256 GB y 1 TB, info y encoger a la mitad, bien.

Tiempos, medianas de 3 (KVM; "fuera" cuenta desde que se lanza QEMU; "reloj" es la marca del kernel en la línea
`fin`):

| | info, fuera | info, reloj | encoger, fuera | encoger, reloj |
|---|---|---|---|---|
| qboot, rtc=off | 1,91 s | 1,27 s | 1,95 s | 1,31 s |
| qboot, con RTC | 0,72 s | 0,07 s | 0,76 s | 0,12 s |
| contrato, rtc=off | 1,94 s | 1,27 s | 1,95 s | 1,31 s |
| contrato, con RTC | 0,71 s | 0,07 s | 0,75 s | 0,11 s |

Los 1,2-1,3 s de más con `rtc=off` son de QEMU, no del dominio. Sin RTC, el CMOS de microvm lee 0xff, y el kernel ve
el bit "actualizando" puesto y lo espera 1 s antes de rendirse (unas 20.000 lecturas del 0x71). En DashOS el CMOS lee 0
y no espera, así que lo esperable allí es la fila "con RTC": unos **0,07 s de kernel hasta el fin de info** y 0,11 s
para encoger 2 GB a 1 GB (y lo que tarde el monitor en cargar y arrancar, que no está en el reloj del dominio).

En TCG (contrato, rtc=off), el reloj del dominio da 1,5 s (info) y 1,7 s (encoger).

Memoria:

- Con 128, 96 y 64 MB, todo bien (3 de 3, KVM y TCG). El dominio ve 122 MB, 90 MB y 57 MB libres.
- ntfsresize llega a 0,6 MB.
- **Mínimo en QEMU: 11 MB** con el arranque del contrato (12 MB con qboot), para info sobre 2 GB.
- Ojo: ntfsresize guarda un bit por cluster del volumen. Con clusters de 4 KB son 32 MB por TB, y solo se tocan las
  páginas de las zonas con datos. En los volúmenes de prueba (casi vacíos) no se nota. En uno lleno de 1 TB serían
  unos 32 MB, y en uno de 3 TB no cabría en 128 MB. No está medido con un volumen lleno.

### Lo que toca el kernel (para el monitor)

`tools/dominio_x86.py accesos contrato [intel]`: un arranque en TCG con las trazas de QEMU (puertos y MMIO) y un
complemento de TCG propio que apunta cada RDMSR/WRMSR. El resultado queda en `obj/accesos/accesos*.txt`.

- **Consola**: escrituras en 0x3F8 y lecturas de 0x3FD. Al iniciarse escribe en 0x3F9-0x3FC: IER 0, FCR 0, LCR 0x83
  y luego 0x03, MCR 0x03, DLL/DLM. Lee 0x3FB.
- **PIC**: escribe 0xFF en 0xA1 y 0xFB en 0x21, y lee 0x21. Si lee 0xFF, decide que no hay PIC ("Using NULL legacy
  PIC").
- **PIT**: escribe en 0x43, 0x40 y 0x42, y lee y escribe el 0x61. Además, **lee el 0x42 100.004 veces** en la
  calibración rápida del TSC. Esa calibración corre aunque haya `tsc_early_khz`, salvo en Intel con CPUID 0x16, y se
  hace antes de buscar el PIC. Con 0xFF en el 0x42 da todas esas vueltas; con cualquier otro valor (p. ej. 0x00),
  falla a la segunda lectura.
- **CMOS**: 0x70/0x71 para leer la hora. Con 0 en el 0x71, pocas lecturas; con 0xFF, 20.000 y 1 s.
- **0x80**: tres escrituras (retardo de E/S).
- **LAPIC** (0xFEE00000):
  - ID 0x20, versión 0x30, TPR 0x80, EOI 0xB0, SVR 0xF0, ISR/TMR/IRR 0x100-0x270 (leídos una vez), ESR 0x280.
  - ICR 0x300: una vez 0x400F6, una IPI a sí mismo con el vector 0xF6, la de irq_work. Hay que entregarla.
  - LVT del temporizador 0x320; LVTPC 0x340; LINT0 0x350 (0x10700, ExtINT enmascarado); LINT1 0x360 (0x400, NMI);
    LVTERR 0x370.
  - En TCG, que no tiene plazo del TSC, también la cuenta inicial 0x380, la actual 0x390 y el divisor 0x3E0.
  - Con KVM y el plazo del TSC, el LVTT queda en 0x400EC (modo plazo, vector 0xEC) y el temporizador va por el MSR
    0x6E0. Se vio con `info lapic` a mitad de un arranque sin disco: el `/init` espera 2 s con nanosleep y el reloj
    funciona.
- **IOAPIC** (0xFEC00000): selector en +0x00 y datos en +0x10. Lee el ID (0), la versión (1; 0x170020, 24 pines) y
  las 24 entradas, y las enmascara todas. Solo programa la del disco: vector 0x20, flanco, activa alta, física,
  destino 0. Lo de flanco y activa alta es lo de ISA, porque la tabla MP dice "como el bus". Sin el registro EOI del
  IOAPIC: con flanco basta el EOI del LAPIC. En QEMU recibió 12 interrupciones en un info.
- **virtio-mmio**: magia, versión 2, aparato 2, fabricante, funciones y una cola de 256. Lee y reconoce las
  interrupciones en +0x60/+0x64. Notifica en +0x50, aunque no sale en la traza: QEMU lo atiende por ioeventfd.
- **MSR** (los de un arranque; con otra CPU pueden ser otros):
  - Los dos fabricantes: APIC_BASE 0x1B (leído), SYSENTER 0x174-0x176 (escritos a 0), EFER, STAR, LSTAR, FMASK,
    FS_BASE, GS_BASE, KERNEL_GS_BASE, TSC_AUX (0xC0000080-0xC0000084, 0xC0000100-0xC0000103), PAT 0x277 (leído) y
    0x8B (revisión del microcódigo).
  - AMD además: CSTAR 0xC0000083, 0xC0010000-0xC0010003 y 0xC0010007 (contadores), VM_CR 0xC0010114 y DE_CFG
    0xC0011029 (escrito).
  - Intel además: 0x34 (SMI_COUNT), FEAT_CTL 0x3A, 0xC2, PLATFORM_INFO 0xCE, MISC_FEATURES 0x140, 0x186/0x187
    (contadores) y 0x64E.
  - Con KVM entran también IA32_TSC_DEADLINE 0x6E0 y, si la CPUID lo anuncia, TSC_ADJUST 0x3B.
- **CPUID** (lo que tiene que anunciar el monitor):
  - **ARAT** (CPUID 6, EAX bit 2): sin él, el kernel exige el PIT aunque haya plazo del TSC.
  - El plazo del TSC (CPUID 1, ECX bit 24).
  - Si el monitor pone el bit de hipervisor (CPUID 1, ECX bit 31), el kernel no intenta cargar microcódigo.

### Lo que no se pudo

- No se pudo poner en QEMU el aparato virtio en 0xd0000000 con la IRQ 5: microvm tiene sus huecos fijos. Se probó en
  0xfeb00e00 con la IRQ 12, que la tabla MP del contrato también lleva a su pin.
- No se pudo poner en QEMU un CMOS que lea 0: microvm sin RTC lee 0xFF. Se midió también con su RTC.
- No se pudieron ver los MSR con KVM: sin root no hay trazas de KVM. Los MSR son de TCG, que no tiene plazo del TSC
  y toma el camino de la calibración del LAPIC. Lo del plazo del TSC se comprobó con KVM mirando el estado del LAPIC.
- No se midió ntfsresize con un volumen lleno grande (la memoria del mapa de clusters), ni con un NTFS hecho por
  Windows: hibernado, o con el registro sin vaciar de verdad. Lo sucio se probó con la bandera puesta a mano.
- Sala limpia: para saber qué toca el kernel invitado se leyó su código (Linux: `tsc.c`, `i8253.c`, `i8259.c`,
  `apic.c`, `rtc-mc146818-lib.c`) y el de ntfsresize. No se copió nada a DashOS; lo que pasa al monitor son hechos
  (puertos, registros, valores).

## 2026-11-17: el monitor y el instalador

### El monitor (la pieza común)

- `caja-linux/vmm/src/x86/` (sin DashOS, sin `unsafe`, `cargo test`): `arranque.rs` (el bzImage por el protocolo de 32
  bits: parte protegida en 1 MB, página cero con la cabecera del propio bzImage, e820, línea e initramfs arriba; y la
  tabla MP con una CPU, el bus ISA y el IOAPIC 1, las 16 de ISA a sus pines), `apic.rs` (APIC local con ISR/IRR, EOI,
  LVT, ICR para la IPI a sí mismo; IOAPIC con 24 entradas, nivel con IRR remoto y flanco) y `maquina.rs` (MMIO al
  APIC, al IOAPIC y a los virtio-mmio con su pin; puertos: COM1 por líneas, PIT, puerto B y CMOS a cero, lo demás
  todo unos; MSR: la base del APIC, lo demás #GP). Reúsa el virtio-mmio y el virtio-blk de la caja Linux de ARM.
  Pruebas: el bzImage hostil (corto, sin "HdrS", protocolo viejo, que no cabe), las sumas de la tabla MP, una línea
  por nivel con EOI, máscaras, el temporizador, vectores reservados, registros a todo unos, la consola sin fin.
- `usuario/src/dominio_x86.rs` (DashOS): la máquina (llamadas 45 a 48 del hipervisor de x86), la RAM en trozos de
  16 MB (lo más que mide un objeto de memoria) seguidos en el invitado y en el servicio, el disco que da quien llama, y
  el bucle de la vCPU: entrega por los huecos del kernel lo que dice el APIC, el vector del temporizador (el plazo del
  TSC lo lleva el kernel), y atiende LECTURA, ESCRITURA, PUERTO y REGISTRO; acaba con "dominio: fin", la muerte de la
  máquina o el plazo.
- El kernel de DashOS no cambia. Lo único fuera de mi zona: el permiso "hipervisor" también al instalador
  (`inicio_x86_uso.rs`) y su tope de memoria a 320 MB (`inicio.rs`).
- **Medido** (Windows 11 de 50 GB, NTFS con 13,9 GB usados, KVM en shiva): `info` tardaba 7,7 a 10 s porque el kernel
  del invitado lee el 0x42 del PIT unas 100.000 veces (cada una, una salida) cuando lee todo unos; con el PIT a cero,
  3,6 s. `encoger` a 22,9 GB (35.092 clusters que mover, 144 MB): 22,5 s (30,7 s antes del arreglo del PIT).
  ntfsresize usa 1,8 MB.
  El primer arranque de un Linux en el hipervisor de x86 de DashOS salió a la primera.

### El instalador

- `Entorno::encoger` (servicio `instalador`): dos vueltas del dominio con solo la partición de Windows prestada
  (`Prestada`: los accesos fuera de ella dan error al invitado): `info` (si ntfsresize no lo deja, se para sin tocar
  nada) y `encoger:<bytes>` (en seco y, si sale, de verdad). Los bytes son exactamente el tamaño nuevo de la partición
  (ntfsresize deja la copia del sector de arranque en su último sector).
- La entrada de Windows encogida va en la misma escritura de la GPT que la de DashOS (`gpt::con_encogida`), y una GPT
  cortada a medias con la partición encogida en una copia se reconoce como instalación nuestra (prueba con cortes en
  cada sector).
- El pendrive lleva el dominio (`EFI/DashOS/DOMINIO.BZI` y `DOMINIO.CPI`, `make pendrive-x86` si están en
  `DOMINIO_X86`); el cargador los pasa en la zona en vivo.

### Pendiente

- La CPUID 6 con ARAT (lo pide el kernel del invitado para fiarse del temporizador del APIC sin PIT): funciona igual;
  si hiciera falta, es del kernel de DashOS (filtro de CPUID), que no toco.
- Un NTFS de varios TB lleno: ntfsresize guarda 32 MB por TB; con 128 MB de RAM del dominio no cabría uno de ~3 TB.
- Un corte de luz en mitad de ntfsresize puede dejar Windows a medias: es el riesgo propio de encoger NTFS. El
  instalador hace antes la copia de las tablas y del primer MiB de la partición, y lo dice en el plan.

### El banco (`tools/prueba_instalador.py encoger`, en capas sobre el Windows 11 del banco de AmiCachy)

12 de 12:
- Plan de encoger hecho en el PC; en el pendrive, encogido por ntfsresize en el dominio e instalado en el hueco
  ("dominio: fin encoger 0"); las demás particiones de Windows iguales byte a byte; el NTFS encogido bien según
  `ntfsresize --info` en el PC (24,6 GB).
- DashOS arranca por su entrada de la NVRAM y por el menú del firmware; Windows arranca después (captura de su pantalla
  de bloqueo, `encoger-windows-NN.png`) y se apaga limpio, y su NTFS sigue bien (lo que se podría encoger pasa de
  13,8 a 13,4 GB: Windows hizo su comprobación).
- Corte de luz en mitad de ntfsresize (QEMU con `kill -9` al empezar la de verdad): el NTFS se sigue leyendo en el PC;
  al repetir, el instalador se para y dice "Windows tiene una comprobación de disco pendiente: arranca Windows, deja
  que la haga y vuelve" (ntfsresize ya lo marcó para chkdsk): no se toca nada más.
- Los discos base, sin cambios.
- Lo que enseñó: con los VARS de serie del banco (sin la entrada "Windows Boot Manager") el firmware arrancaba DashOS
  en vez de Windows; la prueba parte de una capa con Windows arrancado una vez (`win-arrancada`), como un PC de verdad.

### Pruebas de make (tras unir origin/x86)

`make prueba`, `make prueba-x86` (KVM), `make arranque-x86` con KVM y sin él (TCG) y `make arranque-virt`: todas bien.

### Escenario sin el banco de AmiCachy (`encoger-sintetico`, para otra máquina)

`tools/prueba_instalador.py encoger-sintetico`: un disco de 26 GB hecho aquí (GPT de `pendrive_x86.gpt`, ESP FAT32 con
mtools y un `EFI/Boot/bootx64.efi` de mentira, y un NTFS de 25,9 GB con `mkntfs` y tres ficheros puestos con
`ntfscp`, uno de 64 MB al azar). El instalador en DashOS (pendrive en vivo, KVM) lo encoge con el dominio e instala en
el hueco; en el PC, `ntfsresize --info` (10,9 GB, lo que mide la partición nueva) y los tres ficheros iguales
(`ntfscat`); la GPT con sus dos copias iguales; DashOS arranca por su entrada de la NVRAM. 8 de 8 en shiva (AMD, KVM):
`info` 0,4 s y encoger 0,9 s en el dominio.

Para que valga en otra máquina: `tools/dominio_x86.py preparar` baja también el código de Linux de kernel.org (con su
suma) si no está; el banco usa los OVMF_VARS del sistema si no está el de AmiCachy (`OVMF_CODE` y `OVMF_VARS` se
pueden dar), crea su carpeta y no mira los discos base si no están.

### Sin FUSE (pegaso: el AppArmor de Ubuntu no deja a fusermount3 montar sobre un fichero)

`tools/banco_instalador.py`: con `BANCO_SIN_FUSE=1`, o si montar falla (se dice: "sin FUSE: ... se copia a un crudo
disperso"), mirar una capa o una partición es una copia cruda dispersa (`qemu-img convert -O raw -S 4k`, con
`--image-opts` y `offset`/`size` para la partición), que `soltar` borra; escribir en una capa (`Editar`, para preparar
las hostiles) es escribir en esa copia y volver a hacer la capa encima de su misma base (`qemu-img convert -B`; queda
más grande que lo cambiado, 133 MB por un sector en la prueba, pero con su base). `tools/prueba_instalador.py` ya no
llama a qemu-storage-daemon por su cuenta: todo por el banco. `encoger-sintetico` en shiva: 8 de 8 con FUSE y 8 de 8
con `BANCO_SIN_FUSE=1`; `Editar` probado de las dos formas.
