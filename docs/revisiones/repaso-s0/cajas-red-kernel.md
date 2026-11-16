# Repaso contra S0 "Amigable de 2026, con fondo para informáticos": cajas, red, controladores y kernel

Del 2026-11-15. Solo lectura de `docs/diseño/` en el worktree `revision`. No se cambió nada del repo.

Las dos preguntas:

- **A**: ¿lo usa sin ayuda alguien que no es informático?
- **B**: ¿puede un informático ver cómo funciona y cambiarlo sin leer el código? (Shell, guiones, ajustes como
  ficheros de texto en `ENVARC:`, registros legibles.)

Alcance de la lectura: S4, S7, S25, S11, S13, S12, S12b, S35, S21, S24, S17 e `hilos-de-usuario.md`, enteros. De S18,
los apartados 1, 3 (niveles, velocidad), 5, 9, 10 y el resumen. De S1, S2, S3 y S16, solo lo que toca al usuario
(buscado por palabras y leído el pasaje). Un hallazgo "no lo dice" significa que el documento no lo nombra; puede
estar resuelto en otro diseño (S27, S6b, S14) que no es de este grupo y no se miró.

Total: 59 hallazgos.

---

## S4. La caja Amiga

Cumple en lo principal: "No hay discos Amiga falsos" y "Los volúmenes y asignaciones son los de DashOS" (apartado 4).
Los ficheros de un programa Amiga se ven. Las ventanas son ventanas normales (apartado 3).

1. **La ROM la pone el usuario, sin decir dónde ni qué pasa si falta.** Apartado 6: "DashOS no lleva ninguna ROM. La
   pone el usuario, como en Amiberry o en WinUAE". Falla **A**. Cambio: un cajón fijo y con nombre para las ROM, que
   se llena arrastrando el fichero. Si a un programa le falta una biblioteca, aviso en llano ("Este programa necesita
   icon.library, que DashOS aún no trae") con el detalle a un clic. Sin ROM, lo de serie tiene que seguir sirviendo.
2. **Las cajas aparte no dicen quién las pide ni cómo.** Apartado 5: "Cajas aparte para programas dudosos o que fallan
   mucho". Falla **A** y **B**. Cambio: una opción en el menú del icono ("Abrir aparte"), que se guarda como tooltype
   del icono, y la misma opción en la orden de Shell que lanza el programa. Tras una caída, el sistema lo propone solo.
3. **El perfil de la caja solo lo ve `Paq`.** "Decidido el 2026-11-15": "`Paq` le pregunta su perfil y la caja dice lo
   que tiene de verdad". Falla **B**. Cambio: una orden de Shell que imprima ese mismo perfil (procesador, FPU, RTG,
   memoria), para saber por qué un paquete eligió una variante.
4. **El ajuste "memoria amiga" no dice qué ve el usuario cuando se acaba.** Mismo apartado: "con la memoria del ajuste
   memoria amiga". Un programa 68k que topa recibe un `AllocMem` fallido y nada más. Falla **A**. Cambio: cuando la
   caja llega a su tope, aviso en llano ("Los programas Amiga han llegado a su memoria, 256 MB") con un botón para
   subirla. Ver también S3, hallazgo 2.
5. **"Van demasiado rápido" sin salida para el usuario.** Apartado 7: "Programas que cuentan ciclos de CPU para medir
   el tiempo. Con el JIT van demasiado rápido". Falla **A**. Cambio: enlazar con los perfiles de velocidad de S18 y
   ofrecerlos desde el menú del icono; hoy el texto solo dice que quedan fuera.
6. **Contradicción interna, no del principio.** El apartado 2 y "Cómo encaja" dicen "Cada tarea 68k es un hilo del
   kernel"; lo decidido el 2026-11-14 dice "un solo hilo de emulación por caja". S2 repite lo viejo. Falla **B** en el
   sentido de "se entiende entero": quien lee no sabe cuál vale. Cambio: una nota arriba en el apartado 2 de S4 y en
   S2 ("Dentro de la caja Amiga").

## S7. Software de Linux

1. **El disco de la caja no se ve desde DashOS.** "Decidido el 2026-11-15: qué distribución": "la caja es una máquina
   virtual con su disco". Solo se comparte `Home:` ("Dentro se ve como `/home/<usuario>`"). Lo que un programa de
   Linux guarda fuera de su casa (`/etc`, `/var`, lo instalado con pacman, los contenedores) queda en un disco que el
   documento no dice dónde está ni cómo se mira. Choca con S0: "No hay almacenamiento interno opaco". Falla **A** y
   **B**. Cambio: decir en qué fichero vive ese disco, y montarlo de solo lectura como un volumen con nombre (por
   ejemplo `Linux:`) que se abre desde el Workbench y el Shell.
2. **No hay ninguna orden de Shell de DashOS para la caja.** Todo el documento habla de ventanas, lanzador y Ajustes.
   Arrancar, parar, ver el estado o ejecutar un programa de Linux desde un guion de DashOS no aparecen. S0: "Si una
   función solo existe en una ventana, está sin acabar". Falla **B**. Cambio: una orden en `C:` con estado, arrancar,
   parar y "ejecutar esto dentro", que devuelva la salida y el código de error al guion.
3. **Instalar un programa de Linux pide terminal.** "Lo que trae la caja de serie": "Arch Linux ARM, con pacman",
   "Flatpak de serie, con Flathub", y solo "aparece en el lanzador de DashOS y en el escaparate de Hangar"; en "Cómo
   encaja": "Hangar puede enseñarlos". Enseñar no es instalar. Falla **A**. Cambio: dejar escrito que Hangar instala
   y quita los Flatpak con un clic, sin nombrar Linux ni la caja. pacman queda para quien abra la terminal.
4. **Pasar de perfil "terminal" a "completa" es trabajo de informático.** "Dos perfiles": "Pasar de terminal a
   completa es instalar el escritorio dentro de la caja". Falla **A**. Cambio: un solo botón en Ajustes (y su orden)
   que lo haga entero. El instalador decide el perfil por la memoria y no pregunta.
5. **El perfil y la memoria de la caja no son un fichero de texto.** Mismo apartado: "El perfil se elige en Ajustes
   (categoría Caja Linux, S27)"; la memoria la decide `dashos::maquina::memoria_invitado` (256, 512 MB o 1 GB), en el
   código. Falla **B**. Cambio: un fichero en `ENVARC:` con perfil, memoria y distribución; el valor de serie sigue
   saliendo de la máquina, y el fichero lo puede pisar.
6. **"La distribución se puede elegir" sin decir cómo.** "Qué distribución lleva la caja", último punto. Si es una
   pregunta al instalar, falla **A**; si no hay forma escrita, falla **B**. Cambio: nunca se pregunta; Arch de serie.
   Cambiarla es una línea en el fichero de arriba y una orden que rehace la caja.
7. **Si Linux mata un programa por memoria, DashOS no se entera.** La caja tiene 256 MB a 1 GB y dentro manda el
   kernel de Linux. Un Firefox que se queda sin sitio desaparece sin aviso de DashOS. El documento no lo trata. Falla
   **A** y **B** (no deja rastro). Cambio: la caja pasa a DashOS ese suceso; aviso en llano ("Firefox se cerró porque
   la caja Linux se quedó sin memoria") y línea en el registro.
8. **No hay registro legible de la caja.** No se nombra dónde va la consola del kernel invitado ni por qué no arrancó
   una caja. Falla **B**. Cambio: la consola virtio va a un fichero de registro con nombre, y la orden del hallazgo 2
   lo enseña.

## S17. La caja PowerPC

Es análisis; casi todo es interno. La idea de una sola caja con dos motores cumple **A**: el usuario hace doble clic
y no sabe si el programa es 68k o PPC.

1. **OS4 y MorphOS enteros con QEMU piden saber de máquinas.** Apartado 4: "La edición de AmigaOne XE corre en la
   `amigaone` de QEMU con el cargador BBoot; la de Pegasos II, en `pegasos2`"; fases: "Poco código; empaquetado y
   ajustes". El usuario tendría que elegir máquina, cargador y NVRAM. Falla **A**. Cambio: la aplicación reconoce el
   disco de instalación y elige la máquina sola. Los ajustes de cada máquina, en un fichero de texto junto a su disco,
   en un cajón con nombre en `Home:`.
2. **No dice qué ve el usuario cuando un programa PPC no puede correr.** Apartado 1: faltan AltiVec, la SPE "queda
   fuera", OS4 "pide demasiada API". Falla **A** y **B**. Cambio: aviso en llano ("Este programa es para AmigaOS 4 y
   DashOS aún no lo ejecuta") y, en el detalle y en el registro, la instrucción o la biblioteca que faltó.

## S25. La tarjeta gráfica para la caja Linux

Casi todo es interno y de seguridad. No pide nada al usuario: cumple **A** en eso.

1. **La GPU se reinicia y se niegan páginas sin dejar rastro.** Camino C: "si un trabajo del invitado no acaba, DashOS
   reinicia la GPU y se lo dice al invitado como un reinicio"; "Estado: un fallo del invitado lo deja a él sin GPU".
   No dice que se apunte en ningún sitio. Falla **B**. Cambio: cada reinicio por plazo y cada entrada rechazada de la
   tabla sombra, a un registro legible con la caja y la hora.
2. **No se puede ver quién tiene la GPU ni si un programa pinta con ella o con la CPU.** "La GPU es de la caja mientras
   la usa"; el canal tiene "dámela", "suéltala" y "reiníciala", pero solo entre servicios. Falla **B**. Cambio: una
   orden que diga quién tiene la GPU y cuánto lleva gastado del "presupuesto por caja"; y un ajuste de texto para
   forzar el dibujo por CPU, para depurar.

## S11. Interfaz de programación

Cumple en lo de fondo: bibliotecas por nombre, puertos ARexx, tooltypes, catálogos, documentación en castellano e
inglés.

1. **Los puertos de guiones existen, pero no son de serie en cada programa.** Tabla de la recomendación: "Puertos
   ARexx: Igual, sobre canales". No obliga a que el kit dé un puerto a toda aplicación. S0 pide que lo del ratón se
   pueda hacer por guion. Falla **B**. Cambio: el kit con modelo de MUI crea solo un puerto por aplicación, con sus
   menús como órdenes (MUI ya lo hacía), sin que el programador escriba nada.
2. **No dice dónde guardan sus ajustes las aplicaciones nativas.** Se nombran "Iconos y tooltypes" y "Prefs", pero no
   el formato. Falla **B**. Cambio: la API de ajustes del kit escribe texto legible en `ENVARC:<programa>`, y esa es
   la única vía de serie.
3. **El contrato con el kernel es privado y no hay forma de mirar qué hace un programa.** "Al kernel, solo a través
   de una biblioteca": "El contrato entre esa biblioteca y el kernel es privado". Falla **B**. Cambio: una herramienta
   de traza de serie, al estilo de SnoopDos, que enseñe las bibliotecas que abre un programa, sus ficheros y sus
   canales, con salida de texto.
4. **MUI y ReAction de 68k dependen de que el usuario los tenga.** "ReAction... Si el usuario tiene las clases 68k
   instaladas"; "MUI 68k, el del usuario". Un programa MUI recién bajado no abre. Falla **A**. Cambio: `Paq` trata
   MUI como dependencia: si la licencia deja repartirla, la instala; si no, aviso en llano que dice qué falta y de
   dónde sale.
5. **Dos sitios de ajustes de aspecto.** "Material Design": "Los programas MUI de 68k en la caja usan su MUI y sus
   preferencias. Se les puede preparar una configuración". Choca con "Un solo sitio para los ajustes". Falla **A**.
   Cambio: la configuración parecida a Material 3 viene puesta de serie, y Ajustes lleva una entrada que abre las
   preferencias de MUI.

## S13. Red

Cumple: DHCP y mDNS de serie, cortafuegos cerrado sin preguntar, y "órdenes equivalentes en `C:`" para `ifconfig` y
`ping`.

1. **Los ajustes de red se nombran como ventana, no como fichero.** "Se configura en `Prefs/Red`" (cortafuegos) y
   "como opción en `Prefs/Red`" (DNS cifrado). No dice fichero en `ENVARC:` ni orden. Además el nombre no es el de S0
   ("Un solo sitio para los ajustes (S27)"). Falla **B**. Cambio: decir el fichero de texto (reglas del cortafuegos,
   DNS, IP fija) y que la ventana y una orden de Shell escriben ese mismo fichero.
2. **El Wi-Fi solo se describe como servicio y claves cifradas.** "Wi-Fi": "Las claves de las redes se guardan
   cifradas en el cajón del administrador". No hay orden para listar redes o conectarse, ni lista legible de redes
   conocidas. Falla **B**. Cambio: la lista de redes conocidas, en texto; solo la clave va al almacén cifrado. Y una
   orden para ver redes, conectar y olvidar.
3. **No hay rastro de lo que la red hace.** El documento no nombra ningún registro: ni lo que corta el cortafuegos,
   ni qué programa habla con quién. S0: "Nada pasa sin dejar rastro que se pueda leer". Falla **B**. Cambio: un
   registro legible de lo cortado y una orden que liste las conexiones abiertas por programa.
4. **El permiso de escuchar no tiene texto.** "Una aplicación que quiere escuchar en un puerto lo pide como permiso
   aparte". Tal cual, es jerga. Falla **A** si el aviso sale así. Cambio: fijar el texto en llano ("X quiere que otros
   equipos se conecten a este ordenador"), con el puerto en el detalle.
5. **La caja Linux es una caja negra para la red.** "Sus conexiones cuentan como de una sola aplicación" y "una red
   con traducción de direcciones". No se ve qué programa de dentro sale, y un servidor de dentro no se alcanza sin
   "puertos publicados" (S7). Falla **B**, y **A** para quien solo quiere que su programa se vea en la red de casa.
   Cambio: los puertos publicados, en el fichero de ajustes de la caja y con un botón; y en el registro, al menos el
   destino de cada conexión de la caja.

## S12. Controladores de la Pi 4

Casi todo es la lista de controladores y sus fuentes; no toca al usuario.

1. **Un controlador que se reinicia no deja rastro.** "Cómo es un controlador": "Si se cuelga, el gestor de servicios
   lo reinicia, y sus clientes reintentan". Falla **B**, y **A** si se repite (el ratón se para y nadie dice por
   qué). Cambio: cada reinicio, al registro con el motivo; si pasa varias veces seguidas, aviso en llano ("El USB
   está fallando").
2. **No hay forma de ver qué controlador lleva cada aparato.** El documento da a cada uno "las capacidades justas",
   pero no una vista de eso. S0: "se puede ver qué asignaciones, permisos y memoria tiene cada programa". Falla **B**.
   Cambio: una orden que liste aparatos, su controlador, su estado y sus llaves (registros, interrupción, DMA).
3. **Contradicción entre documentos.** La "Decisión del 2026-11-07" sigue diciendo "una capa que imita las partes del
   kernel de Linux... como DDE en Genode". S35 lo da por "Descartado". No lleva nota. Ver S12b.

## S12b. Controladores de Linux con LKL

1. **Contradice a S35 y no lo avisa.** "Recomendación aceptada: B, LKL" (un proceso "linux" con Linux como
   biblioteca). S35, del 2026-11-15, pone "Compilar controladores de Linux dentro de un proceso de DashOS con una capa
   que imite al kernel" como "Descartado" y pasa a máquinas virtuales. `hilos-de-usuario.md` aún justifica los hilos
   con LKL. Falla **B** ("se entiende entero"): quien lee no sabe qué diseño vale ni si LKL entra en lo descartado.
   Cambio: nota de cabecera en S12, S12b e hilos que diga qué sustituye S35 y qué sigue (el Wi-Fi, SACK, `amissl`).

Por lo demás no toca al usuario.

## S35. Dominios de controladores

Cumple en parte: "Si se pasa del tope, se registra y se reinicia el dominio"; Paq con "Deshacer la última
actualización" como única novedad a la vista; rutas sin versión.

1. **Un dominio no se deja mirar.** Seguridad, capa 4: "El dominio solo ve su aparato y su canal con DashOS"; es un
   Linux "Sin distribución" y sin disco. No hay consola, ni lista de dominios, ni se dice dónde se "apunta" el máximo
   de memoria. S0: "El sistema se deja mirar". Falla **B**. Cambio: el registro del kernel de cada dominio sale por
   su canal a un fichero legible de DashOS; una orden lista los dominios con su aparato, memoria usada, tope y
   reinicios; el máximo medido se guarda en texto.
2. **No hay camino para cambiar o añadir un controlador sin romper la seguridad.** Capas 1 y 2: "Imagen firmada y de
   solo lectura", "Sin módulos cargables". Quien tenga un aparato que nuestra imagen no trae no puede hacer nada. S0:
   "Quien quiera sustituir una pieza lo hace en su capa o con un modo de desarrollo explícito, no apagando la
   seguridad". Falla **B**. Cambio: un modo de desarrollo explícito: una imagen de dominio propia, en la capa del
   usuario, firmada con su clave y marcada a la vista. Las otras cinco capas siguen puestas, y se vuelve atrás con las
   generaciones.
3. **"Dicho claro al usuario" apunta a un aviso con jerga.** "Aparatos de la placa en la Pi": "dominio sin esa
   barrera, dicho claro al usuario". Explicar una IOMMU a quien no es informático no sale bien. Falla **A**. Cambio:
   que no sea una pregunta. Se decide de serie; en Ajustes, una línea llana ("El Wi-Fi de esta placa tiene menos
   protección que el resto") con el detalle a un clic.
4. **Prestar un aparato USB no dice quién lo decide.** "DashOS lleva el USB (xHCI) y presta al dominio un aparato
   concreto". Si lo elige el usuario, falla **A**; si es automático y no se ve, falla **B**. Cambio: automático por
   el identificador del aparato, con una tabla de texto (aparato, dominio) que se puede leer y pisar en la capa del
   usuario.
5. **Un dominio que se reinicia en bucle solo deja líneas en el registro.** "Si se pasa del tope, se registra y se
   reinicia el dominio". El usuario ve el Wi-Fi irse cada poco. Falla **A**. Cambio: a la tercera vez seguida, aviso
   en llano y dejar de reiniciar hasta que el usuario diga.

## S18. Los chips del Amiga

Cumple en el disco duro: la "placa propia" hace que "Un cajón de DashOS se ve como un volumen del Amiga".

1. **Tres modos de caja y nadie dice quién elige.** Apartado 1: nativa, máquina entera y mixta; en la nativa "Un
   acceso a `$DFF000` o a las CIA es un error: el Enforcer lo apunta". El usuario abre un juego y no pasa nada. Falla
   **A**. Cambio: la caja nativa, al ver el primer acceso a los chips, ofrece abrir el programa como Amiga completo y
   lo recuerda en el icono (tooltype). Los ADF y los WHDLoad van directos a máquina entera.
2. **Sin Kickstart no hay juegos recién instalado.** "Máquina entera": "El Kickstart lo pone el usuario"; el ROM de
   AROS solo "podría arrancar" y queda como "extra" (pregunta 8). Falla **A**. Cambio: llevar de serie el ROM de AROS
   para que un juego arranque sin poner nada, y pedir el Kickstart solo cuando ese juego falle, con aviso en llano y
   un cajón donde soltarlo.
3. **Memoria, velocidad y exactitud son ajustes de emulador.** "Chip RAM desde 0 (512 KB, 1 MB o 2 MB), slow RAM",
   perfiles "A500", "A1200" y "Rápido: un múltiplo configurable", disquete con velocidad "real por defecto... Rápida
   como opción". Es la pantalla de configuración de UAE. Falla **A**. Cambio: la configuración sale sola por juego
   (de la cabecera de WHDLoad o de una tabla por huella del disco) y solo se toca si algo va mal.
4. **No dice dónde se guarda la configuración de una máquina.** Mismo apartado. Falla **B**. Cambio: un fichero de
   texto junto al juego (o los tooltypes de su icono) con modelo, memoria, velocidad y Kickstart, que la ventana
   escribe y un guion puede cambiar. Y una orden para lanzar una máquina con ese fichero.
5. **Las partidas guardadas quedan en una copia que no se sabe dónde está.** "El disquete": "El disco escribe en una
   copia, no en el fichero original, salvo que el usuario lo pida". Lo mismo con "Guardar el estado". Choca con "ni
   datos de un programa que el usuario no pueda encontrar". Falla **A** y **B**. Cambio: la copia y los estados van
   al lado del disco, con nombre claro, o a un cajón con nombre en `Home:`; el documento tiene que decir cuál.

## S21. Cada trabajo a su chip

Cumple **A**: el usuario no elige nada; "Si el chip no gana, se queda la CPU".

1. **No se puede ver ni forzar qué chip hace cada trabajo.** Regla 3: "va al panel de medidas"; la columna "Reserva"
   decide sola. Un panel es una ventana. Falla **B**. Cambio: una orden que diga, por trabajo, quién lo hace ahora y
   por qué cayó a la reserva; las medidas, también en texto; y un ajuste en `ENVARC:` para forzar la CPU en un trabajo.

## S24. Ordenadores ARM

1. **Instalar en una CM5 o en un Mac pide poner antes otro firmware.** Tabla y "Orden recomendado": "con rpi5-uefi da
   UEFI, GOP y USB"; "Arranca con m1n1 y U-Boot de Asahi". Son pasos de informático. Falla **A**. Cambio: la imagen de
   DashOS para la CM5 lleva ese firmware dentro; en el Mac, el instalador hace ese paso. El usuario solo graba y
   arranca.
2. **Lo que no funciona en una máquina no se le dice a nadie.** "Un portátil": "el teclado y el panel táctil del
   portátil, el Wi-Fi y la batería son controladores propios"; en la RTX Spark, la GPU "se queda en la pantalla del
   firmware". Falla **A** y **B**. Cambio: una lista llana en Ajustes ("En este ordenador aún no funciona: Wi-Fi,
   batería") y la misma en una orden de Shell.
3. **La línea de la CPU solo sale al arrancar.** "El kernel dice al arrancar qué CPU es y si admite big-endian en
   EL0". Bien para **B**, pero sale por la consola de arranque. Cambio: que quede en el registro de arranque legible y
   en la orden que describe la máquina. `DASHOS_PANTALLA` es una variable del cargador: decir en qué fichero de texto
   se pone.

## S1. Arquitectura del kernel

Mirado solo "Qué hace el kernel" y "Qué queda fuera".

1. **Las llaves de un proceso no se pueden listar desde fuera.** "Capacidades: cada objeto del kernel... se usa a
   través de una llave con derechos concretos". No se nombra una forma de mirar qué llaves tiene un proceso. S0: "se
   puede ver qué asignaciones, permisos y memoria tiene cada programa". Falla **B**. Cambio: una llamada de solo
   lectura que liste las llaves de un proceso (clase y derechos), para quien tenga la llave de ese proceso, y una
   orden de Shell encima.

## S2. Núcleos y planificador

Cumple **A** en lo principal: "la ventana se oscurece y ofrece forzar salida"; la temperatura avisa en el escritorio.

1. **El gestor de tareas es solo una ventana.** "Matar procesos": "Un gestor de tareas lista procesos, consumo de CPU
   y memoria, y permite los dos pasos. Es una aplicación más". Falla **B**. Cambio: lo mismo en órdenes de `C:`, como
   `Status` y `Break` del Amiga: listar con CPU y memoria, pedir cierre y forzarlo.
2. **Las tareas de una caja Amiga no salen en ningún sitio.** "Dentro de la caja Amiga": se puede matar una tarea
   68k, pero no dice desde dónde. Si una tarea en bucle "bloquea la caja", el usuario solo puede matar la caja entera.
   Falla **A** y **B**. Cambio: el gestor de tareas y su orden enseñan las tareas de cada caja bajo la caja, y dejan
   cerrar una sola.
3. **Marcar un programa "de fondo" no dice cómo.** "Clase justa": "el usuario puede marcar un programa como de
   fondo". Falla **B** (y no se sabe si **A**). Cambio: en el menú de la ventana, como tooltype del icono y como
   opción de la orden que lanza.
4. **El perfil de energía no dice dónde vive.** "Lo que queda abierto", 2: "responder" o "ahorrar". Falla **B**.
   Cambio: una línea en un fichero de `ENVARC:`, que Ajustes escribe.

## S3. Memoria

Cumple en lo que ya decide: "se dice en el escritorio quién y por qué, y se le da un momento para guardar"; sin
matar al azar.

1. **El registro del Enforcer no dice dónde está ni cómo se lee.** "La caja Amiga": "avisar de toda escritura en la
   página 0... con la tarea y la instrucción culpables... Forja lo puede mostrar"; pregunta 3: "con avisos a un
   registro". Falla **B**. Cambio: un fichero de texto por caja en un sitio dicho, una línea por aviso, y una orden
   que lo siga en vivo. No depender de Forja para leerlo.
2. **"memoria amiga" solo se puede cambiar desde la ventana.** "Cupos como topes": el tope cambia "cada vez que el
   centro de control le dice el ajuste (solo se le acepta al centro, donde lo elige el usuario; `sesiones` no puede
   leer los Prefs del usuario)". Editar el fichero a mano o desde un guion no tiene efecto. Además lo llama
   `Prefs/cajas`, no `ENVARC:` ni S27. Falla **B**. Cambio: el ajuste es un fichero de texto en `ENVARC:`; quien
   vigila ese fichero (el centro o un servicio suyo) avisa a `sesiones`, y una orden de Shell pasa por el mismo
   camino.
3. **Un programa que topa con el cupo de su grupo no se distingue de una máquina sin memoria.** "Los topes": la
   llamada "da `E_MEMORIA`". El programa dirá "sin memoria" con RAM libre. Falla **A** y **B**. Cambio: el kernel
   avisa a `sesiones` de qué grupo topó; aviso en llano ("X ha llegado a su tope de memoria") y línea en el registro
   con el grupo y el tope.
4. **El intercambio a disco "se puede activar" sin decir cómo.** "Poca memoria": "Se puede activar en un disco USB".
   Falla **B**. Cambio: una línea en `ENVARC:` y su casilla en Ajustes.
5. **El aviso de memoria baja va "al registro" sin nombre.** "Primera versión: el aviso llega y se apunta en el
   registro". Falla **B** si ese registro no es un fichero legible. Cambio: decir qué fichero, con la hora y lo libre.

## S16. Un kernel muy seguro

Casi no toca al usuario. Es coherente con S0 en "errores, nunca pánicos" y en "todo recurso tiene dueño y cupo".

1. **Los cupos se cuentan pero no se enseñan.** Apartado F: "Todo objeto que crea se cobra ahí... Al agotarse, la
   llamada devuelve `E_SIN_MEMORIA`". No dice cómo se mira lo cobrado a cada grupo. (S3 nombra `grupo_uso`, pero solo
   para `sesiones`.) Falla **B**. Cambio: la orden de tareas (S2, hallazgo 1) enseña por grupo lo usado y el tope.
   También hay dos nombres para el mismo error: `E_SIN_MEMORIA` aquí y `E_MEMORIA` en S3 e hilos.

## hilos-de-usuario.md

Es la interfaz del kernel; el usuario corriente no la ve.

1. **El motivo del fin de un proceso existe, pero no se dice que se apunte.** "Fin de un hilo": un fallo "termina el
   proceso entero, con fin fallo"; `proceso_salir` distingue "salió con este código" de "lo mataron". Falla **B** si
   se queda dentro del kernel. Cambio: cada fin que no sea normal va al registro con programa, hilo, clase de fallo y
   dirección; el escritorio lo dice en llano ("X se cerró por un error") con ese detalle a un clic.
2. **Un programa con muchos hilos se lleva la máquina y no se ve por qué.** "El cupo del grupo": "un proceso con
   cuatro hilos que calculan recibe cuatro partes"; pregunta 3: "64 hilos... 64 partes". Falla **A** (el sistema va
   lento sin motivo a la vista) y **B**. Cambio: la orden de tareas enseña los hilos de cada proceso; y cerrar la
   pregunta 3 con reparto por grupo antes de abrir los hilos a programas de terceros.

---

## Los cinco hallazgos más importantes del grupo

1. **La caja Linux es opaca por dentro** (S7, 1 y 2; S13, 5). Su disco no se ve desde DashOS y no hay ninguna orden
   de Shell para ella. Cambio: el disco como volumen de solo lectura con nombre, y una orden en `C:` para estado,
   arrancar, parar y ejecutar dentro.
2. **Los juegos del Amiga no sirven recién instalado** (S4, 1; S18, 1 a 3). Hay que poner una ROM, elegir entre tres
   modos de caja y saber de chip RAM y velocidades. Cambio: ROM de AROS de serie, la caja pasa sola a máquina entera
   al ver el primer acceso a los chips, y la configuración sale sola por juego, guardada en texto junto a él.
3. **Los dominios de controladores ni se dejan mirar ni se pueden cambiar** (S35, 1 y 2). Imagen firmada, sin
   consola y sin módulos. Cambio: su registro del kernel a un fichero legible, una orden que los liste con memoria y
   tope, y un modo de desarrollo explícito con imagen propia en la capa del usuario.
4. **Hay ajustes que solo valen desde su ventana y con nombres distintos** (S3, 2; S13, 1; S7, 5; S18, 4). "memoria
   amiga" solo se acepta al centro de control; la red está en `Prefs/Red`; las cajas, en `Prefs/cajas` y en "Ajustes
   (categoría Caja Linux)". Cambio: cada uno, un fichero de texto en `ENVARC:` que manda; la ventana y una orden
   escriben ese fichero.
5. **Lo que se para o se reinicia no deja rastro ni aviso en llano** (S12, 1; S25, 1; S35, 5; S7, 7; S3, 3; hilos,
   1). Controladores reiniciados, la GPU reiniciada por plazo, Linux matando por memoria, un programa que topa con su
   cupo. Cambio: un registro legible común con programa, motivo y hora, y aviso en llano solo cuando el usuario lo
   nota o se repite.

Aparte, una contradicción entre documentos que conviene cerrar: S12 y S12b (y los hilos) siguen dando por aceptada
la capa en proceso con LKL, y S35 la da por descartada.
