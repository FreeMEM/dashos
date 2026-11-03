# S6. Seguridad

## El problema

El principio 2 de S0 dice que DashOS es seguro por defecto: cada aplicación ve solo lo que se le da. Además, el
usuario ha decidido tres cosas:

- **Usuarios, grupos, propietarios y permisos como en Linux.**
- **Cada proceso tiene su PID**, y para matarlo hay que ser su dueño o administrador.
- **Matar siempre funciona** (S2).

Y el kernel es un micronúcleo con capacidades (S1). Hay que decir cómo encajan los permisos de Linux con las
capacidades, cómo se aíslan las aplicaciones, cómo se firman los paquetes y qué se puede hacer con el arranque de la
Pi 4.

## Dos capas que no compiten

- **Quién**: el usuario y sus grupos. Lo decide el modelo de Linux. Sirve para los ficheros y para saber quién
  puede tocar qué proceso.
- **Qué programa**: cada aplicación tiene sus propios permisos, aunque la lance el mismo usuario. Es lo que hacen
  Android y macOS, y lo que Linux de escritorio no hace: allí cualquier programa del usuario puede leer todos sus
  ficheros.

Las capacidades del kernel son el mecanismo de las dos. Un proceso nuevo no tiene nada: solo lo que su lanzador le da
(lo mismo que en Fuchsia, donde tampoco hay "autoridad ambiental", es decir, nada que se pueda pedir solo por saber
su nombre).

## Usuarios, grupos y permisos

- **Cada proceso lleva su usuario y sus grupos**, fijados por el kernel al crearlo. No los puede cambiar él solo.
- **Cada canal entre procesos lleva la identidad de quien lo abrió**, como `SO_PEERCRED` en Linux. Un servidor, como
  el sistema de archivos, sabe siempre quién le habla sin fiarse de lo que el cliente diga.
- **El sistema de archivos comprueba dueño, grupo y permisos** de lectura, escritura y ejecución en cada apertura,
  como Linux (S5).
- **Los servicios del sistema corren como usuarios propios**: el del sonido, el de la red, el del compositor. Si uno
  cae en malas manos, no tiene los permisos de los demás.

### PID y matar procesos

- El kernel da a cada proceso un PID único mientras vive. Hay un servicio de procesos que los lista con su dueño, su
  consumo y su programa.
- Pedirle al servicio que mate un PID funciona si eres su dueño o administrador. El servicio comprueba la identidad
  del canal y usa la capacidad de terminar que tiene sobre todos los procesos (S2).
- Los procesos del sistema solo los puede parar un administrador.

### Sin "root" todopoderoso

En Linux, `root` lo puede todo. Si un programa consigue ser `root`, el sistema es suyo.

- **En DashOS hay administradores, no un superusuario.** Un administrador es un usuario del grupo `admin`.
- **Para hacer algo de administrador hay que confirmarlo con la contraseña**, en una ventana que dibuja el sistema, no
  el programa. Es lo que hacen macOS y polkit. La confirmación da una capacidad para esa acción concreta, no un
  permiso para todo.
- **`SYS:` no lo cambia ni un administrador.** Solo las actualizaciones del sistema, firmadas (S8, S14). Un
  administrador puede instalar paquetes en `Paquetes:`, pero no tocar el sistema base a mano.

## Permisos de cada aplicación

### Lo que una aplicación tiene sin preguntar

- Su propio cajón de datos, por ejemplo `Home:Datos/<aplicación>`.
- Leer su programa y sus bibliotecas.
- Abrir ventanas y recibir el teclado y el ratón cuando tiene el foco.
- Los ficheros que el usuario le entregue a través del selector de ficheros.

### El selector de ficheros como llave

El selector de ficheros (la ventana de "Abrir") no es parte de la aplicación. Lo dibuja un proceso del sistema. Cuando
el usuario elige un fichero, el sistema le da a la aplicación una capacidad para ese fichero y para nada más. Así, un
editor de texto puede abrir cualquier documento que el usuario elija, pero no puede leer los demás a escondidas.
Flatpak lo llama "portales", y en la literatura de seguridad se llama "powerbox".

Lo mismo vale para arrastrar un fichero a una ventana o abrirlo con doble clic desde Workbench.

### Lo que una aplicación tiene que pedir

- Red.
- Cámara, micrófono y ubicación.
- Ficheros fuera de su cajón sin pasar por el selector, por ejemplo un gestor de copias de seguridad.
- La clase de plazo del planificador (S2).
- Puertos ARexx de otros programas.
- Actuar sobre otras ventanas: capturas de pantalla, automatizar el teclado.

Cada paquete declara lo que necesita en sus datos (S8). La primera vez que lo usa, el sistema pregunta al usuario.
El usuario puede revisarlo y quitarlo en `Prefs`.

### La caja Amiga y los programas de Linux

- **La caja Amiga es una sola aplicación** para estos permisos. Los programas 68k esperan poder leer todo el disco y
  hablar entre sí, así que dentro de la caja no hay más aislamiento que el del usuario. Por defecto, la caja principal
  ve `Home:`, `Work:` y los volúmenes del usuario, y tiene red.
- **Las cajas aparte** (S4) pueden ir más cerradas: sin red, o con un solo cajón. Es la forma de probar un programa
  68k dudoso.
- **Los programas de Linux** van con permisos parecidos a los de Flatpak: su cajón, lo que el usuario elija y lo que
  declaren (S7).

## Firmas

- **Paquetes y repositorios firmados con Ed25519**, al estilo de signify o minisign. Es más sencillo que el GPG que
  usa pacman, y las claves son cortas.
- **Claves de confianza**: la del repositorio de DashOS, la de Hangar y las que añada un administrador.
- **Las actualizaciones del sistema** van firmadas por la clave de DashOS. El arranque comprueba la firma de la
  instantánea de `SYS:` antes de usarla (S14).

## El arranque de la Pi 4

La Pi 4 tiene arranque verificado:

1. La ROM del chip lleva una clave de Raspberry Pi y comprueba el firmware de la EEPROM.
2. La EEPROM comprueba que la clave del dueño coincide con un resumen grabado en la OTP (memoria que solo se puede
   escribir una vez).
3. Con esa clave comprueba `boot.img`, una imagen FAT con el kernel, contra su firma `boot.sig`.

Tiene tres pegas:

- **Es irreversible.** Grabar la OTP no se deshace. Si se pierde la clave, la placa solo arranca lo ya firmado.
- **No hay un enclave seguro** que guarde claves privadas dentro del chip.
- **Solo las revisiones B1 y C0 del BCM2711** permiten un arranque seguro completo.

Recomendación: **no activarlo de serie**. Documentarlo como opción para quien lo quiera. DashOS verifica por su
cuenta desde el kernel en adelante: el kernel comprueba la firma de la instantánea de `SYS:`, y el sistema de
archivos comprueba cada bloque con sus sumas (S5). Sin arranque verificado, alguien con la tarjeta en la mano puede
cambiar el kernel; pero un programa malicioso no puede cambiar el sistema.

## Otras defensas

- Rust en el kernel y en los servicios.
- Direcciones aleatorias, y nada escribible y ejecutable a la vez (S3).
- Sin intercambio a disco, así que las claves en memoria no acaban en la tarjeta (S3).
- Cortafuegos que cierra las entradas por defecto (S13).
- Registro de seguridad: inicios de sesión, confirmaciones de administrador, permisos concedidos y avisos del
  Enforcer (S3).

La A72 de la Pi 4 no tiene las defensas por hardware de CPUs más nuevas, como PAN, PAC, BTI o MTE (S3). La A76 de la
Pi 5 añade PAN, pero tampoco tiene PAC, BTI ni MTE. Lo que se hace a mano en su lugar está en S16.

## Lo que queda abierto

1. ¿Sin superusuario: administradores que confirman cada acción con contraseña, y `SYS:` intocable salvo por
   actualizaciones? Recomendado: sí.
2. ¿Permisos por aplicación preguntados en el primer uso, como macOS, o todos al instalar, como el Android antiguo?
   Recomendado: en el primer uso.
3. ¿Arranque verificado de la Pi 4 solo como opción documentada, porque es irreversible? Recomendado: sí.

## Cómo encaja

- Con S0: es el principio 2.
- Con S1: todo sale de las capacidades y de que un proceso nuevo no tiene nada.
- Con S2: matar con PID, si eres dueño o administrador; la clase de plazo es un permiso.
- Con S3: direcciones aleatorias, sin intercambio y el Enforcer.
- Con S4: la caja es una aplicación; las cajas aparte pueden ir cerradas.
- Con S4b: el cajón de datos de cada aplicación vive en `Home:`.
- Con S5: dueño, grupo, permisos y sumas de comprobación.
- S7 aplica permisos tipo Flatpak a los programas de Linux.
- S8 declara los permisos de cada paquete y verifica firmas.
- S13 pone el cortafuegos.
- S14 comprueba la firma de `SYS:` al arrancar.

## Fuentes

- Arranque seguro de la Pi 4: [arquitectura de seguridad de Raspberry Pi](https://www.raspberrypi.com/documentation/security/security-architecture.html),
  [libro blanco "Raspberry Pi 4 Boot Security"](https://pip.raspberrypi.com/categories/685-whitepapers-app-notes/documents/RP-004651-WP/Raspberry-Pi-4-Boot-Security.pdf),
  [rpi-eeprom](https://github.com/raspberrypi/rpi-eeprom),
  [usbboot #93](https://github.com/raspberrypi/usbboot/pull/93/files) y
  [guía de Embedded Computing](https://embeddedcomputing.com/technology/open-source/board-system-standards/how-to-enable-secure-boot-on-raspberry-pi-4).
- Capacidades sin autoridad ambiental en Fuchsia: [principios originales](https://fuchsia.dev/docs/contribute/contributing-to-cf/original_principles)
  y [modelo de aislamiento](https://fuchsia.dev/docs/get-started/learn/intro/sandboxing).

No se comprobó nada en una placa. Flatpak, polkit, macOS, signify y minisign se citan por su funcionamiento conocido.
