# S35. Aprovechar los controladores de Linux: dominios de controladores

Decidido con el usuario el 2026-11-15. Generaliza lo que S32 (controladores de serie) y S34 (gráficas) ya dicen. Es
diseño: no hay nada hecho salvo lo que se nombra como existente.

> **Decidido por el usuario el 2026-11-16 (S36).** Los controladores de Linux van en máquinas virtuales, como dice
> este documento. LKL (S12b: Linux como biblioteca dentro de un proceso de DashOS) se retira. El porqué, en "Por qué
> máquinas virtuales y no LKL".

## El problema

Escribir a mano los controladores de Wi-Fi, Bluetooth, gráficas, impresoras o tarjetas raras no es realista. Linux
los tiene. La pregunta es cómo usarlos sin meter Linux dentro de DashOS, sin mezclar licencias (GPL frente a MPL) y
sin gastar memoria de más.

## La idea: DashOS ya habla virtio

En QEMU, DashOS usa red, disco, sonido y gráfica virtuales (virtio), con controladores hechos y probados. Un **dominio
de controladores** es una máquina virtual pequeña con un Linux mínimo que lleva el aparato de verdad con su controlador
de Linux y se lo ofrece a DashOS **como un aparato virtio**. DashOS no necesita un controlador nuevo: usa los que ya
tiene. Los papeles se invierten (DashOS manda y es el hipervisor, pero consume aparatos que le sirve su invitado).

## Tres caminos, cada uno para lo suyo

| Qué | Cómo | Por qué |
|---|---|---|
| Lo estándar y siempre encendido: NVMe, AHCI, xHCI y USB básico, sonido HDA, red con cable de Intel y Realtek, pantalla del firmware | **Controladores propios** de DashOS | Son pocos, pequeños y documentados. Memoria extra: cero |
| Lo difícil: Wi-Fi, Bluetooth, gráficas (S34), impresoras y escáneres, tarjetas raras | **Dominio de controladores** con Linux | El controlador ya existe y se mantiene solo |
| Compilar controladores de Linux dentro de un proceso de DashOS con una capa que imite al kernel (como DDE de Genode o LinuxKPI de FreeBSD) | **Descartado** | Menos memoria, pero hay que adaptar cada controlador y seguir cada versión de Linux; y enlaza código GPL |

Un controlador propio sustituye al del dominio cuando exista. Una máquina con poca memoria funciona sin ningún dominio.

## Por qué máquinas virtuales y no LKL (decidido el 2026-11-16)

S12b había aceptado LKL: el kernel de Linux compilado como biblioteca dentro de un proceso de DashOS. Comparados:

| | LKL (S12b) | Dominio en máquina virtual (este documento) |
|---|---|---|
| Qué Linux corre | Una variante fuera del kernel oficial (`arch/lkl`), mantenida aparte | El kernel oficial, sin tocar |
| Lo que hay que escribir | La capa que pide `arch/lkl` y, además, cómo llegan a ese Linux el bus, las interrupciones y el DMA del aparato de verdad | Pasar el aparato a la máquina y servirlo por virtio |
| Lo que DashOS ya tiene | Hilos en los procesos | El hipervisor (la caja Linux) y los controladores virtio, probados |
| Seguir las versiones de Linux | Depende de que el proyecto LKL siga | Se cambia el kernel y ya |
| Frontera de licencia | Código GPL enlazado con nuestra capa, en la zona GPL | Un kernel aparte que solo habla virtio: la más limpia |
| Protección del propio Linux | La de un proceso | La de un proceso y, además, el código del kernel protegido desde fuera (capa 3) |
| Memoria | Sin medir | Medido en QEMU: 15 a 17 MB en reposo para red y Wi-Fi juntos (ver "Memoria") |

La razón que pesa: LKL se pensó para usar la pila de red y los sistemas de ficheros de Linux dentro de un programa,
no para llevar aparatos de verdad. Lo que S12b quería ahorrar (escribir la unión con cada bus) habría que escribirlo
igual, y sobre un Linux que no es el oficial. Comprobado: LKL sigue fuera del kernel oficial (las propuestas para
meterlo llegan hasta 2021). No comprobado: si hoy admite varios procesadores o aparatos reales; lo que se recuerda es
que no, pero no se ha verificado.

Lo que sigue de S12b: lo de red (SACK, `amissl.library` nativa). Los hilos de usuario hacen falta igual, aunque ya
no por LKL. La medida del paso 1 (ver "Orden" y "Memoria") salió muy por debajo de 48 MB, así que no hay que volver
a mirarlo. LKL no tendría por qué gastar menos: es el mismo kernel.

## Cómo llegan los aparatos al dominio

- **Aparatos USB: sin IOMMU.** DashOS lleva el USB (xHCI) y presta al dominio un aparato concreto (un Wi-Fi USB, una
  impresora, un escáner). Vale en todas las máquinas, también en la Raspberry Pi. Es lo primero que se hace.
  Qué aparato va a qué dominio se decide solo, por el identificador del aparato: el usuario no elige nada. La tabla
  (aparato, dominio) es un fichero de texto que se puede leer. Dónde vive y quién puede cambiar una línea, por diseñar
  al hacerlo.
- **Aparatos PCI: con IOMMU** (VT-d, AMD-Vi; SMMU en ARM). Sin IOMMU, un aparato pasado alcanza toda la memoria: no se
  hace. DashOS x86 aún no lleva IOMMU (X5b en la rama `x86`).
- **Aparatos de la placa en la Pi** (el Wi-Fi por SDIO): la Pi no tiene IOMMU. O controlador propio (el de Broadcom es
  "fullmac": casi todo lo hace su firmware), o dominio sin esa barrera. Sigue por decidir con la placa delante,
  también después de retirar LKL (era lo primero de S12b). Si se
  elige el dominio, no se pregunta al usuario: va de serie, y Ajustes lo dice en una línea llana ("El Wi-Fi de esta
  placa tiene menos protección que el resto"), con el detalle a un clic.

## Dos Linux distintos, y por qué no se juntan

| | Linux de controladores | Linux de aplicaciones (la caja Linux, S7) |
|---|---|---|
| Qué lleva | Kernel, el controlador y un programa que lo sirve por virtio. Sin distribución | Una distribución entera con los programas del usuario |
| De quién es | Del sistema: lo instala y lo actualiza DashOS | Del usuario: instala lo que quiera |
| Cuándo corre | Solo si hay un aparato que lo necesita; algunos, a ratos (la impresora, al imprimir) | Cuando el usuario abre un programa de Linux |
| Confianza | Código conocido, encerrado | No es de fiar para el sistema |

No se juntan aunque ahorraría memoria: por el de controladores pasan la red y, en las gráficas, la pantalla; si los
programas del usuario vivieran en el mismo Linux, uno malicioso lo espiaría todo (S34). Y si el usuario rompe su
distribución, no se queda sin Wi-Fi. **Ahorro que sí se hace**: los dos pueden usar el mismo kernel, cargado una vez y
compartido en solo lectura.

## Seguridad: seis capas

1. **Imagen firmada y de solo lectura.** Kernel, controladores y firmware van en un fichero que DashOS comprueba antes
   de arrancarlo (forma parte de `SYS:` o de un paquete firmado). El dominio no tiene disco donde escribir: cada
   arranque es limpio y no hay nada que un atacante pueda dejar instalado.
2. **Sin módulos cargables.** Los controladores van compilados dentro del kernel (`CONFIG_MODULES=n`). No se puede
   cargar uno nuevo en marcha.
3. **El código del kernel, protegido desde fuera.** DashOS es el hipervisor: marca el código de ese Linux como no
   modificable y sus datos como no ejecutables en la segunda etapa de traducción. Aunque alguien tome el dominio, no
   puede cambiar su kernel (como pKVM en Android).
4. **Lo mínimo.** El dominio solo ve su aparato y su canal con DashOS. Ni ficheros ni memoria del sistema.
5. **DashOS no se fía de él.** Lo que llega del dominio se valida como llegado de fuera. Si el de Wi-Fi cae, lo peor
   que hace es comportarse como una red hostil: por eso las conexiones van cifradas de extremo a extremo (TLS, S13).
6. **Alarma de comportamiento** (idea del usuario): ver "Memoria".

Un controlador de Wi-Fi lee datos hostiles del aire. En Linux corre dentro del kernel; aquí, encerrado en una máquina
virtual. Con los mismos controladores, DashOS queda más protegido que Linux.

## Los dominios se dejan mirar

Encerrado no quiere decir escondido (S0). Tiene que existir:

- **Una lista.** `Status` (S2) enseña los dominios junto a los controladores propios (S12): el aparato de cada uno, la
  memoria que usa, su tope y cuántas veces se ha reiniciado. El Monitor del sistema enseña lo mismo.
- **Su registro.** Lo que dice el kernel de cada dominio sale por su canal con DashOS a un fichero de texto en
  `Estado:Registros/` (S6), uno por dominio. La orden `Registro` lo lee y lo sigue en vivo. El dominio sigue sin
  consola y sin disco: solo saca texto.
- **Cada reinicio**, una línea en ese registro con el dominio, el motivo y la hora.
- **El máximo de memoria medido** en esta máquina (ver "Memoria") se guarda en texto que se puede leer.

El formato de todo esto, por diseñar al hacerlo.

### Un aparato que nuestra imagen no trae

Quien tenga un aparato así necesita una salida que no sea apagar la seguridad. Tiene que existir una imagen de dominio
propia, y solo en el modo de desarrollo (S6): se activa a propósito, se ve una marca fija en pantalla, queda en
`Estado:Registros/` y vive en la capa del usuario o de la máquina, sin tocar `SYS:`. Las seis capas no se cambian. Cómo
se firma y se comprueba esa imagen es una decisión de seguridad: por diseñar al hacerlo, con revisión (S36).

## Memoria: tope medido y dinámico

Preocupación del usuario: el consumo. Reglas:

- **Sin número fijo en el código.** Cada imagen de dominio lleva, firmado, lo que mide en nuestro banco. En la máquina
  del usuario se afina: se apunta el máximo que usa de verdad (depende del aparato y del número de procesadores) y el
  tope queda en ese máximo más un margen. Con cada actualización del kernel se mide de nuevo.
- **Si se pasa del tope**, se registra (en `Estado:Registros/`, ver "Los dominios se dejan mirar") y se reinicia el
  dominio (menos de un segundo, sin estado). La memoria no es una prueba de integridad (un módulo maligno puede ocupar
  lo mismo): es una alarma, la sexta capa. Si pasa tres veces seguidas, sale un aviso en llano ("El Wi-Fi está
  fallando y se reinicia solo"), con el detalle a un clic.
- **Condición para dar el diseño por bueno**: medirlo. Objetivo: 48 MB o menos para el dominio de red y Wi-Fi.
  **Medido el 2026-11-16 en QEMU: se cumple, con margen.** Red y Wi-Fi juntos arrancan con 15 a 17 MB y en reposo
  usan 14 a 17 MB, según el número de procesadores (detalle abajo, en "Medido"). Lo que falta por medir lo sube algo,
  no lo triplica. Los de gráficas siguen sin medir.
- **Lo que baja el consumo**: controladores propios para lo siempre encendido; dominios a demanda; Linux sin
  distribución (el kernel del dominio ocupa de 4 a 5 MB en disco, 2 MB comprimido); el globo de memoria, que ya existe
  (aviso-cajas); el kernel compartido entre dominios (su código y sus datos de solo lectura son 4 a 5 MB por dominio).
- **Con poca memoria**: el dominio de red entra si su tope medido cabe (unos 24 MB; 48 en 1 GB caben). La Pi 3
  (1 GB) y el THEA1200 (512 MB) dependen del Wi-Fi, así que no vale decir "con 1 GB o menos, sin dominios", como
  decía aquí antes. Los de gráficas, desde 4 GB y apagables (S34).

### Medido (2026-11-16)

**Qué se midió.** Linux 7.3.4, la estable de kernel.org de ese día (comprobada con su `sha256sums.asc`), sin tocar.
`make tinyconfig` y encima la configuración propia de `caja-linux/dominios/` (`comun.config`, una por arquitectura y
una por dominio), sin módulos. Sin distribución: el initramfs solo lleva `/init`, un programa propio (4 KB en
aarch64, 9 en x86-64) sin biblioteca C (`caja-linux/dominios/servidor.c`) que levanta las interfaces, abre un socket
de paquetes en cada una (lo que usaría para pasar tramas a DashOS), dice "listo" y vuelca `/proc/meminfo` tres
segundos después.

- **Red**: la tarjeta con cable e1000e que emula QEMU (Intel 82574L), por PCIe, y virtio-net como canal hacia fuera.
- **Wi-Fi**: `cfg80211`, `mac80211`, `mac80211_hwsim` (una radio simulada, para que la pila trabaje sin aparato:
  QEMU no emula ningún Wi-Fi) y `rt2800usb` (las llaves USB Ralink/MediaTek RT5370, RT3070) con su xHCI. Este
  último está dentro del kernel pero sin aparato: cuenta su código, no lo que gasta con una llave enchufada.
- **Red y Wi-Fi juntos**, que es el dominio del objetivo.

**Dónde.** En QEMU 11.1, no en hardware: aarch64 con `-M virt` y Cortex-A72 **emulado** (sin KVM: el PC es x86) y
x86-64 con `-M q35` y KVM. Las dos tarjetas, en un concentrador de QEMU sin salida al PC.

**Cómo.** `tools/medir_dominios.py todo`. La memoria mínima es la menor `-m` (en MB) con la que llega a "listo" y
vuelca la memoria sin pánico ni OOM, por búsqueda binaria y confirmada dos veces. "Usada en reposo" es la memoria de
la máquina menos `MemFree`, con el mínimo más un 25 %: incluye lo que el kernel reserva al arrancar (en aarch64, 9 a
10 MB contando el propio kernel, según su línea `Memory:`) y lo que pide después. "Con tráfico": tras el reposo,
20.000 tramas de 1.514 bytes de la tarjeta con cable a la virtio-net. El tiempo, de cinco arranques (mediana): desde
que se lanza QEMU hasta que "listo" sale por la consola serie, y el reloj del kernel (`/proc/uptime`) en ese momento.

| Máquina | Dominio | CPU | Mínimo para arrancar | Usada en reposo | Con tráfico | Kernel en memoria | Kernel en disco | "Listo" desde fuera | Reloj del kernel |
|---|---|---|---|---|---|---|---|---|---|
| aarch64 | red | 1 | 13 MB | 12,0 MB | 12,5 MB | 3,8 MB | 4,1 MB (1,6 comprimido) | 0,45 s | 0,37 s |
| aarch64 | red | 4 | 15 MB | 14,3 MB | 14,7 MB | 3,8 MB | 4,1 MB | 0,49 s | 0,40 s |
| aarch64 | Wi-Fi | 1 | 14 MB | 13,4 MB | — | 4,8 MB | 5,1 MB (2,1) | 0,36 s | 0,27 s |
| aarch64 | Wi-Fi | 4 | 16 MB | 15,3 MB | — | 4,8 MB | 5,1 MB | 0,42 s | 0,32 s |
| aarch64 | red y Wi-Fi | 1 | 15 MB | 14,2 MB | 14,5 MB | 4,9 MB | 5,2 MB (2,2) | 0,56 s | 0,46 s |
| aarch64 | red y Wi-Fi | 2 | 16 MB | 15,0 MB | 15,6 MB | 4,9 MB | 5,2 MB | 0,53 s | 0,44 s |
| aarch64 | red y Wi-Fi | 4 | 17 MB | 16,2 MB | 16,4 MB | 4,9 MB | 5,2 MB | 0,56 s | 0,46 s |
| x86-64 | red | 1 a 4 | 27 MB | 14,4 a 15,3 MB | 14,9 a 15,7 MB | 4,8 MB | 1,7 MB (bzImage) | 0,50 a 0,52 s | 0,28 s |
| x86-64 | Wi-Fi | 1 a 4 | 27 MB | 15,2 a 16,0 MB | — | 5,8 MB | 2,1 MB | 0,46 a 0,47 s | 0,12 s |
| x86-64 | red y Wi-Fi | 1 a 4 | 27 MB (17 MB, ver abajo) | 15,7 a 16,7 MB | 17,0 a 17,2 MB | 5,9 MB | 2,2 MB | 0,65 a 0,66 s | 0,30 s |

"Kernel en memoria" es código, datos y bss según la línea `Memory:` del propio kernel; aparte, 0,4 MB (0,8 en x86)
de `init` que se liberan al arrancar. Los objetos del kernel (slab) son 3 a 4,5 MB de lo usado. Los tiempos variaron
poco (lo más lejos de la mediana, 0,76 s en un arranque); el PC tenía otras sesiones (carga de 3 a 10 al acabar cada
fila).

**Lo que se lee de la tabla.**

- **El objetivo de 48 MB se cumple**: el dominio de red y Wi-Fi arranca con 15 a 17 MB y se queda en 14 a 17 MB.
  Con el margen del 25 %, el tope medido sería de unos 20 a 24 MB.
- **Cada procesador más cuesta unos 0,7 MB** (pilas, datos por CPU y slab por CPU): de 1 a 4, unos 2 MB.
- **El tráfico apenas sube** (0,3 a 1,5 MB): los anillos de recepción ya se reservan al levantar la interfaz.
- **x86-64 pide 27 MB para arrancar pero usa lo mismo**: el kernel se descomprime en `CONFIG_PHYSICAL_START`, 16 MB
  de serie, y no puede ir más abajo; la memoria de debajo se usa después. Con `CONFIG_PHYSICAL_START=0x200000`
  arranca con 17 MB, igual que en ARM.
- **El arranque, en medio segundo** hasta "listo", también con el procesador ARM emulado. En x86 la mitad es el
  firmware de QEMU y descomprimir el bzImage. La sonda de la e1000e se lleva de 0,1 a 0,15 s del reloj del kernel.

**Qué opciones mueven la memoria** (aarch64, red y Wi-Fi, usada con 64 MB y 1 / 4 procesadores; base: 14,5 / 16,8 MB):

| Cambio sobre la base | Usada | Mínimo para arrancar | Nota |
|---|---|---|---|
| Partir de `defconfig` en vez de `tinyconfig` (sin módulos) | 106 MB (con 256) | 143 MB | Kernel de 78 MB: lo que más pesa con diferencia |
| `CONFIG_INET=y` (IPv4 en el dominio) | +1,7 / +1,6 MB | +1 MB | El dominio pasa tramas: no necesita IP |
| `-O2` en vez de `-Os` (`CC_OPTIMIZE_FOR_SIZE=n`) | +1,1 / +1,2 MB | +1 MB | +1 MB de código |
| SLUB normal en vez de `SLUB_TINY` (trae `SLUB_DEBUG`) | +0,6 / +2,3 MB | +1 MB | Crece con los procesadores |
| `KALLSYMS=y` | +0,4 MB | = | |
| `NR_CPUS` de serie (512) en vez de 8 | +0,3 MB | = | |
| `BASE_SMALL=y` | 0 / −0,4 MB | = | |
| Registro de 4 KB (`LOG_BUF_SHIFT=12`) en vez de 128 KB | −0,5 MB | −1 MB | Se pierden mensajes del arranque |
| Sin `printk` | −0,9 MB | −1 MB | No vale: el dominio se queda sin registro ("Los dominios se dejan mirar") |

Lo que más baja, con diferencia, es partir de `tinyconfig`: de 106 a 15 MB. Lo demás mueve de 0,3 a 2 MB cada cosa,
y la configuración medida ya lleva lo que baja (`SLUB_TINY`, `-Os`, sin IP, `NR_CPUS=8`, sin `KALLSYMS` ni ayudas de
depuración). Hacen falta sí o sí: `PRINTK` (sin él no hay consola ni registro), `SMP`, `PCI` con MSI, `NET` con
`PACKET`, `DEVTMPFS`, `PROC_FS` y `SYSFS` (el servidor busca ahí sus interfaces), `BLK_DEV_INITRD`, virtio y el
controlador del aparato. En x86-64 se usó además ACPI (sin probar si hace falta).

**Lo que no se midió** (cada cosa sube la cifra; ninguna se acerca a triplicarla):

- **Un Wi-Fi de verdad.** `mac80211_hwsim` mide la pila de Wi-Fi, no un aparato: con una llave enchufada, el
  controlador reserva sus búferes USB y carga su firmware (el de `rt2800usb` es pequeño, unos KB; el de un Wi-Fi PCI
  de Intel, del orden del MB). Tampoco hay tráfico de Wi-Fi.
- **El programa que negocia la clave del Wi-Fi** (`wpa_supplicant` o `iwd`): no va en el dominio medido. Como
  referencia, el `wpa_supplicant` del PC ocupa 3,4 MB de memoria residente (x86-64, glibc dinámica): medido en el PC,
  no en el dominio.
- **El servidor de verdad**: el de la prueba solo abre los aparatos. Servirlos a DashOS como aparato virtio añade lo
  suyo, sin medir.
- **Hardware real**: ni la Pi ni un PC. En aarch64 los tiempos son de un procesador emulado; con KVM o en la Pi
  serán otros. Tampoco el hipervisor de DashOS: aquí es QEMU, y la memoria que gasta QEMU en el PC no cuenta.

Las cifras, con todos los campos de `/proc/meminfo`, salen en `resultados.json` de la carpeta de trabajo
(`DASHOS_MEDIR_LINUX`); los kernels y su `.config`, también allí. Nada de eso va a git: se rehace con el guion.

## Lo que se toma de NixOS (y lo que no)

El usuario pidió mirar cómo trata NixOS lo de "solo lectura" o inmutable.

Cómo lo hace NixOS: cada paquete vive en su propia carpeta de `/nix/store`, con un nombre que incluye una huella de
todo lo que se usó para construirlo; esa carpeta no se modifica nunca. El sistema entero es una "generación": un
conjunto de enlaces a carpetas del almacén. Actualizar es construir la generación nueva y cambiar un enlace, de golpe;
volver atrás es elegir la generación anterior en el arranque. Todo sale de un fichero de configuración declarativo, así
que una máquina se puede rehacer igual. Límites: el almacén se monta de solo lectura, pero el administrador puede
remontarlo y nadie comprueba en marcha que no se haya tocado (salvo en sus imágenes nuevas con dm-verity y arranque
seguro); y su lenguaje y sus rutas con huella son difíciles para el usuario.

Lo que DashOS ya tiene parecido: `SYS:` como imagen firmada de solo lectura con dos ranuras (S14), un cajón por paquete
en `Paquetes:` (S4b), y las asignaciones por proceso, que hacen lo que en NixOS hacen los enlaces.

Lo que se toma:

- **Para los dominios de controladores**: construirlos de forma reproducible (las mismas entradas dan la misma imagen,
  bit a bit), para que la firma diga algo y cualquiera pueda comprobarla. Y lo que NixOS no garantiza y aquí sí: la
  integridad se comprueba al arrancar y el hipervisor la mantiene en marcha.
- **Para Paq, decidido (detalle en S8)**: generaciones como en NixOS, pero con asignaciones en vez de enlaces.
  Cada versión de un paquete en su propio cajón (en un almacén que solo toca `Paq`), sin pisar la anterior; una
  "generación" es la tabla de qué cajones entran en `C:`, `LIBS:`, etc. Instalar o actualizar es escribir cajones
  nuevos y cambiar la tabla de golpe; volver atrás es usar la tabla anterior; los cajones que ninguna generación usa
  se limpian. Dos programas pueden usar versiones distintas de una biblioteca sin conflicto, porque cada proceso tiene
  su vista. El usuario lo aceptó con una condición: que sea elegante y no le dé trabajo. Por eso las rutas que se
  ven no llevan versión (`Paquetes:<nombre>`, como siempre) y lo único nuevo a la vista es "Deshacer la última
  actualización".
- **Estado aparte**: lo que cambia vive solo en `Home:` y `Estado:`; `RAM:`, `ENV:` y `T:` se pierden al salir. Es la
  "impermanencia" de NixOS, que DashOS ya sigue.
- **Más adelante**: describir una máquina en un fichero (usuarios, programas, ajustes) para rehacerla igual. Los
  ajustes en ficheros de texto de `ENVARC:` (S27) ya lo acercan.

Lo que no se toma: el lenguaje Nix, las huellas dentro de las rutas que ve el usuario (rompen las rutas al estilo
Amiga) y generar toda la configuración como enlaces.

## Orden

1. ~~Medir un Linux mínimo con un controlador (memoria y tiempo de arranque) en QEMU.~~ **Hecho el 2026-11-16**: 15
   a 17 MB y medio segundo para red y Wi-Fi juntos; el objetivo de 48 MB se cumple (ver "Memoria", "Medido").
2. Prestar un aparato USB a un dominio y recibir de él red por virtio (vale en todas las máquinas, se prueba en QEMU).
   **En parte, el 2026-11-16** (`docs/bitacoras/dominio-red.md`): la red por virtio hecha y probada (DHCP y TCP de
   DashOS por un dominio, con hostiles), pero con una tarjeta PCI pasada sin IOMMU, solo en QEMU, como paso
   intermedio. **Con USB, el 2026-11-16** (`docs/bitacoras/dominio-usb.md`): el xHCI de DashOS presta el aparato
   (un adaptador de red USB) y el dominio lo engancha por USB/IP con `vhci-hcd`, por un par de sockets y una memoria
   compartida en vez de vsock. Sin IOMMU y en cualquier máquina; probado en QEMU con hostiles. La e1000 por PCI se
   retiró de la prueba.
3. Las seis capas de seguridad, con pruebas hostiles (un dominio que intenta escribir su kernel, cargar un módulo,
   salirse de su memoria, mentir por el canal virtio).
4. IOMMU y aparatos PCI (con la rama `x86`).
5. Las gráficas (S34).

## Cómo encaja

S7 (caja Linux: el hipervisor y los aparatos virtio que ya existen), S12 y S12b (controladores), S13 (red, TLS), S14
(`SYS:` firmada), S30 (x86, zona GPL e IOMMU), S32 (qué controladores van de serie), S33 y S34 (vídeo y gráficas), S8
(las generaciones).
