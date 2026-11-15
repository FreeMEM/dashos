# S35. Aprovechar los controladores de Linux: dominios de controladores

Decidido con el usuario el 2026-11-15. Generaliza lo que S32 (controladores de serie) y S34 (gráficas) ya dicen. Es
diseño: no hay nada hecho salvo lo que se nombra como existente.

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

## Cómo llegan los aparatos al dominio

- **Aparatos USB: sin IOMMU.** DashOS lleva el USB (xHCI) y presta al dominio un aparato concreto (un Wi-Fi USB, una
  impresora, un escáner). Vale en todas las máquinas, también en la Raspberry Pi. Es lo primero que se hace.
- **Aparatos PCI: con IOMMU** (VT-d, AMD-Vi; SMMU en ARM). Sin IOMMU, un aparato pasado alcanza toda la memoria: no se
  hace. DashOS x86 aún no lleva IOMMU (X5b en la rama `x86`).
- **Aparatos de la placa en la Pi** (el Wi-Fi por SDIO): la Pi no tiene IOMMU. O controlador propio (el de Broadcom es
  "fullmac": casi todo lo hace su firmware), o dominio sin esa barrera, dicho claro al usuario. Por decidir con la placa
  delante.

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

## Memoria: tope medido y dinámico

Preocupación del usuario: el consumo. Reglas:

- **Sin número fijo en el código.** Cada imagen de dominio lleva, firmado, lo que mide en nuestro banco. En la máquina
  del usuario se afina: se apunta el máximo que usa de verdad (depende del aparato y del número de procesadores) y el
  tope queda en ese máximo más un margen. Con cada actualización del kernel se mide de nuevo.
- **Si se pasa del tope**, se registra y se reinicia el dominio (menos de un segundo, sin estado). La memoria no es
  una prueba de integridad (un módulo maligno puede ocupar lo mismo): es una alarma, la sexta capa.
- **Condición para dar el diseño por bueno**: medirlo. Objetivo: 48 MB o menos para el dominio de red y Wi-Fi. Las
  cifras de hoy (32 a 64 MB para red; más para gráficas) son estimaciones sin medir.
- **Lo que baja el consumo**: controladores propios para lo siempre encendido; dominios a demanda; Linux sin
  distribución (nuestro kernel mínimo ocupa 5 MB en disco); el globo de memoria, que ya existe (aviso-cajas); el kernel
  compartido entre dominios.
- Con 1 GB o menos, sin dominios. Los de gráficas, desde 4 GB y apagables (S34).

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
- **Propuesta para Paq, por decidir (va a S8)**: generaciones como en NixOS, pero con asignaciones en vez de enlaces.
  Cada versión de un paquete en su propio cajón (`Paquetes:<nombre>-<versión>`), sin pisar la anterior; una
  "generación" es la tabla de qué cajones entran en `C:`, `LIBS:`, etc. Instalar o actualizar es escribir cajones
  nuevos y cambiar la tabla de golpe; volver atrás es usar la tabla anterior; los cajones que ninguna generación usa
  se limpian. Dos programas pueden usar versiones distintas de una biblioteca sin conflicto, porque cada proceso tiene
  su vista. Complementa a las instantáneas del sistema de archivos, no las sustituye.
- **Estado aparte**: lo que cambia vive solo en `Home:` y `Estado:`; `RAM:`, `ENV:` y `T:` se pierden al salir. Es la
  "impermanencia" de NixOS, que DashOS ya sigue.
- **Más adelante**: describir una máquina en un fichero (usuarios, programas, ajustes) para rehacerla igual. Los
  ajustes en ficheros de texto de `ENVARC:` (S27) ya lo acercan.

Lo que no se toma: el lenguaje Nix, las huellas dentro de las rutas que ve el usuario (rompen las rutas al estilo
Amiga) y generar toda la configuración como enlaces.

## Orden

1. Medir un Linux mínimo con un controlador (memoria y tiempo de arranque) en QEMU. Sin esto no se sigue.
2. Prestar un aparato USB a un dominio y recibir de él red por virtio (vale en todas las máquinas, se prueba en QEMU).
3. Las seis capas de seguridad, con pruebas hostiles (un dominio que intenta escribir su kernel, cargar un módulo,
   salirse de su memoria, mentir por el canal virtio).
4. IOMMU y aparatos PCI (con la rama `x86`).
5. Las gráficas (S34).

## Cómo encaja

S7 (caja Linux: el hipervisor y los aparatos virtio que ya existen), S12 y S12b (controladores), S13 (red, TLS), S14
(`SYS:` firmada), S30 (x86, zona GPL e IOMMU), S32 (qué controladores van de serie), S33 y S34 (vídeo y gráficas), S8
(la propuesta de generaciones).
