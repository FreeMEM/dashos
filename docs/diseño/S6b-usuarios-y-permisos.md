# S6b. Usuarios y permisos: el diseño concreto

Propuesta del 2026-11-10, aprobada el mismo día (ver "Decisiones"). Se implementa en `docs/REVISION_LOOP.md`.

## El problema

S6 dice qué se quiere: usuarios, grupos y permisos como en Linux; un PID por proceso; matar siempre funciona; sin
superusuario; cada aplicación ve solo lo que se le da. No dice cómo se hace con lo que hay.

Lo que hay hoy (`docs/referencia/kernel.md` y `servicios.md`):

- El kernel no sabe qué es un usuario. Sabe de llaves, con derechos (leer, escribir, transferir) y una **marca** de
  64 bits que el dueño de la llave no puede cambiar.
- Un servicio reconoce a cada cliente por la marca de su llave. Las marcas las pone `inicio`, que es el único que
  escribe sin marca.
- Hay llaves revocables y grupos de recursos que se matan enteros.
- El servicio de archivos guarda dueño, grupo y modo en cada fichero (RedoxFS), pero no los mira: el asa 0 es la raíz
  del disco para cualquiera.
- Los volúmenes cifrados se abren con contraseña (Argon2id) y admiten un código de recuperación.
- Un proceso nace con 8 llaves como mucho.

Este documento baja S6 a piezas que se pueden programar y probar, en fases.

## La idea en cuatro frases

1. **El kernel sigue sin saber de usuarios.** La identidad de un proceso es la marca de sus llaves.
2. **Un servicio nuevo, `sesiones`, reparte las llaves.** Lleva una ficha por proceso: quién es (usuario y grupos) y
   qué programa es (aplicación y permisos).
3. **Tener la llave es tener el permiso.** Una aplicación sin permiso de red no tiene llave del servicio de red. No
   hay nada que comprobar después.
4. **Los ficheros tienen las dos capas.** El servicio de archivos mira dueño, grupo y modo como Linux, y además cada
   programa solo alcanza los cajones que su ficha le da.

## La identidad: en las llaves, no en el kernel

S6 pedía que el kernel fijara el usuario de cada proceso y lo pusiera en cada canal. Hay dos formas de cumplirlo.

**A. El kernel guarda usuario y grupos por proceso** y los sella en cada mensaje.
- Gana: se parece a Linux (`SO_PEERCRED`).
- Pierde: el kernel aprende qué es un usuario, un grupo y un administrador. Es código nuevo en la parte que más
  cuesta auditar, y no resuelve la otra mitad: los permisos por aplicación seguirían fuera.

**B. La identidad es la marca de la llave** y la ficha la guarda quien reparte las llaves.
- Gana: no toca el kernel. La marca ya existe, ya está auditada (auditoría de la fase 4, M6) y ya cumple lo que S6
  quería: el proceso no puede cambiarla. Sirve igual para "quién" y para "qué programa".
- Pierde: quien reparte las llaves es de confianza total. Ya lo es `inicio`.

**Recomendado: B.** Lo que ve el usuario es lo mismo. El kernel no crece.

### La ficha

`sesiones` guarda una por proceso que lanza:

| Campo | Qué es |
|---|---|
| número | 64 bits, no se repite. Es la marca de todas sus llaves y es su **PID** |
| sesión | A qué sesión de usuario pertenece (0: del sistema) |
| usuario | uid, 32 bits |
| grupos | gid principal y hasta 16 más |
| aplicación | Nombre del paquete (S8), o "suelto" |
| perfil | servicio, escritorio, aplicación, caja o suelto (ver "Perfiles") |
| permisos | Los concedidos a esa aplicación para ese usuario |
| grupo de recursos | La llave de grupo del kernel con la que se mata entero |

Cuando `sesiones` da a un proceso una llave de un servicio, antes le manda al servicio, sin marca, un mensaje de
**alta**: la marca, usuario, grupos, perfil, nombre y los derechos que tiene en ese servicio. Es lo que `inicio` hace
hoy con marca y nombre, ampliado. Al morir el proceso manda la **baja**.

### Números

- Usuarios: 0 es "sistema", el dueño de lo que viene de fábrica. **Ningún proceso corre como 0 y el 0 no tiene
  privilegios.** Del 1 al 99, los servicios (sonido, red, compositor...). Desde el 1000, las personas. Por debajo de
  65.536, para que quepan en `fib_OwnerUID` de `dos.library`.
- Grupos: 0 sistema, 10 `admin`. Cada persona tiene un grupo propio con su mismo número. Los demás grupos son para
  compartir cajones.

## Quién es de confianza

| Pieza | Qué puede | Por qué se le confía |
|---|---|---|
| kernel | Todo | — |
| `inicio` | Recibe todas las llaves del kernel. Lanza los servicios base y `sesiones`, y los vigila | Pequeño, sin entrada de fuera |
| `sesiones` | Tiene el extremo sin marca de cada servicio: da de alta clientes. Lleva cuentas, fichas, permisos y procesos | Sin ventanas. Solo lee ficheros suyos y datos de paquetes ya verificados |
| `guardian` | Dibuja la entrada, el bloqueo, la confirmación de administrador y las preguntas de permisos | Recibe contraseñas. No reparte llaves: solo contesta a `sesiones` |
| `selector` | La ventana de "Abrir" y "Guardar". Corre como el usuario y puede ceder asas de ficheros | Solo cede lo que el usuario elige |
| `archivos` | Ve todos los ficheros abiertos | Es quien aplica los permisos |
| compositor | Ve todo lo que se teclea y se dibuja, también las contraseñas | No lee datos de fuera: las imágenes las decodifica cada programa |

Un servicio corriente (sonido, red) no es de confianza para esto: corre con su usuario propio y, como cliente de
`archivos`, solo ve su cajón. El compositor tampoco reparte llaves ni ve ficheros, pero por él pasa el teclado.

Si cae `sesiones` en malas manos, cae todo salvo los cajones cifrados de quien no tiene la sesión abierta.

## Usuarios y sesiones

### Dónde se guardan las cuentas

En un volumen nuevo, **`Estado:`**: lo que el sistema escribe y no es de ningún usuario. `SYS:` no sirve porque no
cambia. Hay que añadirlo a la tabla de S4b.

- `Estado:Cuentas/usuarios`: una línea por usuario: `nombre:uid:gid:nombre completo:volumen:estado`.
- `Estado:Cuentas/grupos`: `nombre:gid:miembros`.
- Dueño: el usuario de `sesiones`. Los demás, solo leer.
- **No hay fichero de contraseñas.** La contraseña de un usuario es la que abre su volumen cifrado. Comprobarla es
  probarla contra las ranuras de claves de ese volumen (Argon2id, medio segundo en una Pi 4).

### El cajón de cada usuario

- Un volumen cifrado por usuario (FS10), que se ve como `Home:`. Raíz del usuario, modo `0700`.
- Al crear la cuenta se da un **código de recuperación** para apuntar en papel (la ranura ya existe en RedoxFS).
- **Un administrador no puede leer el cajón de otro usuario.** Sin la contraseña o el código, el volumen no se abre.
  Lo que sí puede: borrar la cuenta, o poner contraseña nueva con el código de recuperación.
- **Borrar una cuenta se puede deshacer unos días.** La cuenta queda "retirada" (el campo `estado`) y su volumen se
  guarda cerrado y cifrado, sin que nadie lo abra. Pasado el plazo se borra solo. Mientras, un administrador puede
  devolver la cuenta. El plazo, por decidir.
- **Las cuentas también se llevan desde el Shell**: la orden `Usuario` (crear, borrar, devolver, grupos) pide la
  misma acción de administrador que el programa `Usuarios` de `SYS:System` (S4b).

### Entrar, bloquear y salir

1. `guardian` enseña la pantalla de entrada y manda nombre y contraseña a `sesiones`.
2. `sesiones` pide a `archivos` abrir el volumen. Si abre, crea la sesión: un grupo de recursos, el espacio de nombres
   (S4b) y el escritorio, con la ficha del usuario.
3. Bloquear: el compositor tapa todo (S10). Para volver, `sesiones` comprueba la contraseña sin tocar el volumen.
4. Salir: se mata el grupo de la sesión, se revocan sus llaves y se cierra el volumen. La clave se olvida.

Una sesión cada vez en la primera versión. El cambio rápido de usuario cabe después: las fichas ya llevan sesión.

Entrada automática: posible, guardando la clave del volumen en `Estado:`. Quien coja la tarjeta lee el cajón. Hay
que decirlo al activarla. No recomendada de serie (S14 ya proponía pantalla de entrada).

### Primer arranque

Un asistente crea el primer usuario, que es administrador, y su volumen. **Necesita azar de calidad** para las
claves: hoy DashOS no lo tiene (S16, propiedad 5). Es condición previa.

## Ficheros: las dos capas

### Capa 1: dueño, grupo y modo

El servicio de archivos conoce, por el alta, el usuario y los grupos de cada cliente. En cada orden:

- **Recorrer**: hace falta `x` en cada cajón del camino.
- **Leer y escribir**: `r` o `w` en el fichero. Se mira en cada orden, no solo al abrir. `buscar` admite dos bits
  nuevos, "para leer" y "para escribir", y falla ya ahí si no se puede: así `Open()` da el error donde se espera.
- **Crear, borrar y renombrar**: `w` y `x` en el cajón. Con el bit pegajoso, solo borra el dueño.
- **Lo nuevo** nace con el usuario del cliente, el grupo del cliente (o el del cajón si tiene `setgid`) y el modo
  pedido menos la máscara de la sesión (`022`).
- **Cambiar permisos** (orden 15): solo el dueño. **Cambiar el grupo**: el dueño, a un grupo suyo. **Cambiar el
  dueño**: solo con una acción de administrador.
- **El bit `setuid` no hace nada.** En DashOS nadie gana privilegios por ejecutar un fichero.
- La comprobación es una función propia del servicio. No se usan `Node::owner` ni `Node::permission` de RedoxFS, que
  dan paso libre al usuario 0.
- Los bits "no borrar" del Amiga siguen como están, aplicados a dueño, grupo u otros según quién pide.

Hacia `dos.library`: `fib_OwnerUID`, `fib_OwnerGID`, `SetOwner()` y los bits de grupo y otros de `fib_Protection` ya
existen en AmigaOS 3. Se llenan con estos datos.

Los volúmenes sin dueños (FAT, discos del Amiga, ADF): todo aparece como del usuario de la sesión que los montó.

### Capa 2: qué cajones alcanza cada programa

- **Las asas 0 a 15 de cada cliente son sus raíces.** Las pone quien lo lanza. Cada raíz tiene derechos: leer, o leer
  y escribir. El cliente no las puede cerrar ni subir por encima de ellas: "cajón padre" se para en la raíz.
- Ya no hay "raíz del disco para todos". Un cliente sin raíces no ve nada.
- El permiso efectivo es el de la capa 1 **y** el de la raíz por la que se llegó.
- Los nombres (`Home:`, `LIBS:`, `PROGDIR:`...) son cosa del cliente: al nacer recibe una tabla de nombre a raíz y
  subcamino, y la biblioteca resuelve. Una asignación con capas (S4b) es una lista de raíces que se prueban en orden.

Orden nueva en el servicio, **ceder**: un cliente pasa una asa suya a otro cliente, como raíz o como asa normal, con
derechos iguales o menores. Solo la pueden usar los clientes con el derecho de **repartir** en su alta: `sesiones`,
`selector` y el escritorio. Así se hacen:

- **Las raíces de un programa**: `sesiones` abre los cajones y los cede antes de lanzarlo.
- **El selector de ficheros** (S6): el usuario elige, `selector` cede el asa de ese fichero a la aplicación.
- **Doble clic y arrastrar**: el escritorio cede el asa del fichero al programa que lo abre.

Otros derechos del alta: **administrar** (volúmenes, instantáneas, comprobar y reparar; hoy lo pide cualquiera) y
**cambiar dueño**. La contraseña de un volumen deja de ir en el mensaje: va en la ventana, que se pone a cero al
contestar.

### Qué raíces tiene cada uno

| Quién | Raíces |
|---|---|
| Una aplicación | Su cajón `Home:Datos/<paquete>` (escribir), su paquete (leer), `SYS:` y los cajones `Libs`, `Fonts` y `Locale` de los demás paquetes (leer), `RAM:T/<número>` |
| El escritorio y el selector | `Home:` entero, `Work:` y los demás volúmenes, `Paquetes:` y `SYS:` (leer) |
| El Shell y la caja Amiga principal | Como el escritorio. El cajón propio de la caja es `Home:Amiga` (S4b) |
| Un servicio | Su cajón `Estado:Servicios/<nombre>` y `SYS:` (leer) |
| El gestor de paquetes | `Paquetes:` (escribir) |
| Nadie | `SYS:` para escribir. Solo la actualización del sistema |

## Permisos de cada aplicación

### Cómo nace un programa

Nace con **una llave**: su canal con `sesiones`, marcado con su número. Por él recibe la tabla de nombres y pide lo
demás. Las llaves de los servicios que ya tiene concedidos (archivos, compositor) van en la tabla de creación, hasta
7, para no pagar viajes al arrancar.

Lo que se pide por ese canal:

| Petición | Qué hace `sesiones` |
|---|---|
| servicio | Da la llave de un servicio, si la ficha tiene el permiso |
| lanzar | Crea otro proceso: con la misma ficha (un ayudante) o, si quien pide es el escritorio o el Shell, con la de otro programa |
| procesos, matar | Lista y mata (ver "Procesos") |
| administrar | Pide una acción de administrador (ver más abajo) |

Un programa puede seguir creando procesos él solo con `proceso_crear`. Esos hijos no tienen ficha: solo las llaves
que su padre les pase, y mueren con su grupo.

### Pedir un permiso

Cuando la biblioteca necesita un servicio que el programa no tiene:

1. ¿Lo declara el paquete (S8)? Si no, se niega sin preguntar. Queda una línea en el registro ("Editor pidió la red
   y su paquete no la declara") y, si el programa enseña el error, sale en llano con ese detalle a un clic.
2. ¿Ya lo decidió el usuario? Se cumple lo que dijo.
3. Si no, `guardian` pregunta: "Editor quiere usar la red". La respuesta se guarda.
4. Concedido: `sesiones` da de alta al cliente en el servicio y le entrega una llave marcada y **revocable**. La
   copia revocable conserva la marca (mirado en `kernel/src/llamadas.rs`, `llave_revocable`).

Quitar un permiso revoca esa llave en el acto: el programa se queda sin el servicio aunque esté abierto.

### Dónde está lo concedido y cómo se mira (2026-11-15, S36)

Lo concedido se guarda por usuario en `Home:Prefs/Permisos/`, en texto. El dueño es `sesiones` y solo él escribe.
El usuario lo puede leer (antes era modo `0700` y no lo leía ni su dueño): ni el Shell ni ningún programa lo pueden
cambiar. **Este es el único sitio**: es de cada usuario y viaja con su volumen (S4b); `Estado:` es para lo que no
es de ningún usuario. S28 y S10c citan este apartado. Hoy el código guarda los de la guardia de la caja Amiga en el
cajón de `sesiones` (S28); su sitio es este.

- **Con el ratón**: Ajustes, categoría Permisos (S10c, S27). Lista lo concedido a cada programa y lo quita.
- **Desde el Shell**: la orden `Permiso`: listar, dar y quitar.
- **Ver los propios es libre.** Dar o quitar desde el Shell pide la confirmación de `guardian`, igual que la
  pregunta del primer uso: así un guion no se concede nada a escondidas.
- Lo que se da, se quita y se niega queda en el registro de seguridad.

El formato del fichero, por diseñar al hacerlo.

**Hecho** (rama `registro-permisos`, 2026-11-16). `Home:Prefs/Permisos/concedidos`: una cabecera que dice qué es y
cómo se cambia, y una línea por permiso, `programa permiso`. El cajón (`Permisos`) y el fichero son de `sesiones`
(0755 y 0644): el dueño los lee; nadie más los escribe. `sesiones` lo lee al entrar la persona y lo escribe al cambiar
algo (si el volumen está cerrado, cuando entre). Un cajón o un fichero que no es de `sesiones` (uno puesto a mano) no se
lee ni se escribe, y queda en el registro: el servicio de archivos solo da a `sesiones` ese cajón si es suyo
(`HUECO_SUYO`). Lo de los programas del sistema, que no son de nadie, en `Estado:Prefs/Permisos/concedidos` (con el
usuario del sistema en una tercera columna). La orden `Permiso` lista lo propio (libre) y da o quita con la
confirmación de `guardian` ("¿Cambiar un permiso?", con el teclado y la D para los detalles); quitar revoca en el acto.
Pendiente del usuario: el sitio de lo del sistema (`Estado:Prefs/Permisos/`).

### Lista de permisos

| Permiso | Qué da | Lo que lee el usuario ("Editor quiere...") |
|---|---|---|
| `red` | Llave del servicio de red, para conectar | "usar la red" |
| `red-escuchar` | Abrir puertos | "que otros equipos se conecten a este ordenador" |
| `camara`, `microfono`, `ubicacion` | La llave de cada servicio | "usar la cámara", "el micrófono", "saber dónde estás" |
| `ficheros` | `Home:` entero como raíz, sin pasar por el selector | "ver y cambiar todos tus ficheros" |
| `volumenes` | `Work:` y los discos que se enchufan | "usar Work: y los discos que enchufes" |
| `plazo` | La llave de la clase de plazo del planificador | "tener prioridad para que el sonido no se corte" |
| `ventanas` | Capturar la pantalla y mandar teclas a otras ventanas | "ver tu pantalla y escribir en otras ventanas" |
| `arexx:<puerto>` | Hablar con el puerto ARexx de otro programa | "dar órdenes a Tachyon" (el nombre del otro programa) |
| `inicio` | Arrancar solo al entrar en la sesión | "abrirse solo cuando entras" |
| `atajos` | Commodities (S10): pedir al compositor combinaciones de teclas concretas y recibir solo esas | "usar teclas rápidas en todo el sistema" |
| `fondo` | Commodities (S10): saber cuándo no se usa el ordenador (desde 10 s), apagar la pantalla mientras, comportamientos del teclado (sin Bloq Mayús) | "saber cuándo no usas el ordenador y apagar la pantalla" |
| `commodities` | Exchange (S10): ver y manejar las commodities de la misma persona; ClickToFront y AutoPoint | "ver y manejar tus utilidades de fondo" |

Sin pedir nada: su cajón, su paquete, ventanas propias, sonido de salida y lo que el usuario le entregue.

La pregunta nunca enseña el nombre interno del permiso: va en "Detalles", con el puerto o el volumen. Los textos de
la tabla son provisionales; lo que se fija es que cada permiso tiene su frase en llano.

### Perfiles

| Perfil | Quién | Encierro |
|---|---|---|
| servicio | Los del sistema | Usuario propio, su cajón |
| escritorio | Workbench, `selector`, `Prefs` | El usuario entero. Vienen en `SYS:` |
| aplicación | Lo instalado como paquete | Lo de arriba: su cajón y lo que declare |
| caja | La caja Amiga y la de Linux | Una sola aplicación por caja (S6). La principal: `Home:`, `Work:` y red |
| suelto | Un programa sin paquete abierto con doble clic | Como una aplicación sin nada declarado: pregunta todo, con un aviso en llano ("Este programa no viene del catálogo. Se abre encerrado: solo ve lo que tú le des"); que no está firmado va en "Detalles" |

**El Shell es como la caja**: una aplicación con `Home:`, `Work:` y red, y lo que se lanza desde él hereda su ficha.
Quien compila un programa en Dash y lo prueba desde el Shell no se encuentra ninguna pregunta.

**Probar una pieza propia sin quitar el encierro** (decidido el 2026-11-16, S36). Dos cosas que tienen que existir:

- **Modo de desarrollo por aplicación.** El usuario marca una aplicación y esa sí carga de su capa (`Home:Libs`,
  `Home:C`, `Home:Fonts`). Las demás siguen como en la regla 6. Marcarla pide la confirmación de `guardian`: si no,
  la marcaría el mismo programa malo que dejó la biblioteca. La marca se ve en la ficha de la aplicación y queda en
  el registro (`Estado:Registros/seguridad`, S6). Se quita igual.
- **Una orden para lanzar desde el Shell un programa encerrado**, con la ficha que tendría al abrirlo con doble
  clic, para probarlo como lo verá el usuario. Sin ella, lo lanzado desde el Shell hereda `Home:`, `Work:` y red.

Dónde se guarda la marca y el nombre de la orden, por diseñar al hacerlo.

**Hecho** (rama `admin-desarrollo`, 2026-11-16; bitácora `docs/bitacoras/admin-desarrollo.md`). La ficha lleva su
perfil: "libre" (el de siempre, como el Shell) o "aplicación" (encerrado). La orden **`Abrir`** (orden 122 de
`sesiones`) abre un programa instalado (`SYS:Programas`) encerrado, con su propia identidad: solo `SYS:`, su
`PROGDIR:` y su cajón `Home:Datos/<programa>` (que ve como `Datos:` y se crea suyo si falta), sin `Home:`, `Work:` ni
`RAM:`, y sin la capa del usuario en `C:`, `LIBS:`, `FONTS:` y `CLASSES:`; sus ayudantes nacen igual de encerrados y
no puede cambiar permisos. El **modo de desarrollo** es un permiso más de lo concedido, `desarrollo`
(`Home:Prefs/Permisos/concedidos`): se da y se quita con `Permiso DAR|QUITAR <programa> desarrollo` (con la
confirmación de `guardian`) o se quita en Ajustes, Permisos, donde se ve con su frase; un programa no lo puede pedir
como un permiso de primer uso. Con él, la aplicación abierta encerrada tiene además `Home:C`, `Home:Libs`, `Home:Fonts`
y `Home:Classes` solo para leer (`HomeC:`, `HomeLibs:`... delante en su asignación, que `Assign` enseña); el compositor
marca sus ventanas (borde y pestaña "DESARROLLO" en magenta, y "Desarrollo · " en la barra), y cada vez que se abre así
queda una línea en `Estado:Registros/seguridad`. Quitarlo devuelve el encierro al relanzar. Pendiente del usuario: el
nombre `Abrir`, los nombres `Datos:` y `HomeLibs:`, la marca magenta, que el doble clic abra ya encerrado (hoy no:
lanza como el Shell), y el perfil "suelto" (un programa sin identidad propia no se abre encerrado todavía).

## Administradores

- Administrador es quien está en el grupo `admin`. No hay otra diferencia: sus programas corren con los mismos
  permisos que los de cualquiera.
- Una **acción de administrador** es una de una lista cerrada: instalar o quitar un paquete del sistema, crear o
  borrar usuarios, cambiar grupos, cambiar el dueño de un fichero, reparar un disco, crear o borrar volúmenes, matar
  un proceso de otro usuario o del sistema, cambiar la hora o la red del sistema, añadir una clave de confianza.
- El programa la pide a `sesiones`. `guardian` enseña qué programa pide qué, y pide la contraseña de un administrador.
- **La acción la hace el servicio que la tiene a su cargo** (el gestor de paquetes instala, `archivos` repara). El
  programa que la pidió no recibe ningún poder general.
- Cada acción se confirma. Sin "vale durante 5 minutos".
- **Una tanda es una acción** (decidido el 2026-11-16, S36). Varias cosas que se piden juntas (los diez paquetes de
  un plan de `Paq`, un usuario nuevo con sus grupos) van en una sola ventana de `guardian`, que enseña la lista
  entera y pide la contraseña una vez. El servicio hace solo lo que estaba en la lista enseñada; lo demás no se hace
  y pide su propia confirmación.
- Queda apuntada en el registro de seguridad (`Estado:Registros/seguridad`, S6).

**Hecho, la tanda** (rama `admin-desarrollo`, 2026-11-16). Un programa pide una tanda a `sesiones` (orden 120, la
lista en texto en una memoria, `usuario/src/tanda.rs`); `sesiones` la copia y la lee entera antes de preguntar: si una
línea no es de la lista cerrada, nombra un proceso que no existe o la lista pasa de 16 acciones o 4 KB, no se enseña ni
se hace nada. `guardian` enseña la lista en llano, numerada, con la frase del administrador, y pide su nombre y su
contraseña una vez; `sesiones` hace solo esas, en orden, cada una por quien la tiene a su cargo (el comprobador aparte,
`sesiones`), y **se para en la primera que falle**: las de después no se hacen (pueden contar con las de antes) y quien
pidió sabe cuál falló y qué pasó con las demás. Una acción suelta (`ADMINISTRAR`) sale con la misma ventana, como una
tanda de una. Cada acción hecha o fallida, una línea en `Estado:Registros/seguridad`. Desde el Shell, la orden
`Administrar` (`Administrar comprobar "matar 65540" registro`, o `DESDE plan.txt`). Las acciones de hoy son las tres
que existen (comprobar, matar, registro): instalar paquetes y crear usuarios entrarán en la lista cuando existan.

### Que la ventana sea de verdad la del sistema

Un programa puede dibujar una ventana igual que la de `guardian` para robar la contraseña. Defensas:

- Mientras `guardian` pregunta, el compositor no entrega teclado a nadie más ni deja capturar la pantalla.
- La ventana lleva **una imagen o frase que el usuario eligió al crear la cuenta**. Ningún programa la conoce ni la
  puede capturar. Si no sale, no es el sistema.

## Procesos

- El PID es el número de ficha. `sesiones` contesta la lista: PID, usuario, aplicación, nombre y consumo (el del
  grupo de recursos, con `grupo_uso`).
- **La ficha de un proceso propio se puede mirar entera**: perfil, permisos, raíces y tabla de nombres, además de
  lo de la lista. Con las órdenes de procesos (`Status` y `Break`, S6) y en el Monitor del sistema. La de un proceso
  de otro usuario, solo un administrador.
- **Matar**: `sesiones` mata el grupo del proceso si quien pide es del mismo usuario. Si no, es acción de
  administrador. Como el grupo se mata entero, también caen los hijos sin ficha.
- Más adelante, para ver uno a uno los hijos sin ficha: un número de proceso en el kernel y una llamada que liste los
  de un grupo. No hace falta para empezar.

## Si hay un fallo de seguridad

Se da por hecho que habrá fallos. Lo que este diseño decide es **cuánto gana quien explota uno**. Evitar los fallos
del kernel y hacerlos difíciles de explotar es cosa de S16.

No hay un sitio al que escalar de un salto: no hay superusuario, `setuid` no hace nada y ser administrador no es un
estado del proceso. Para subir hacen falta dos fallos: uno en un programa y otro en un servicio de confianza o en el
kernel.

### Si cae esta pieza, qué gana el atacante

| Pieza que cae | Qué gana | Qué no gana |
|---|---|---|
| Una aplicación | Su cajón, los permisos que tenga concedidos y los ficheros que el usuario le cedió | El resto de `Home:`, las demás aplicaciones, contraseñas |
| Un programa del Shell o de la caja Amiga principal | `Home:` y `Work:` del usuario, y la red. Leer la lista de permisos guardados | Otros usuarios, `SYS:`, `Paquetes:`, cambiar los permisos guardados, acciones de administrador |
| El escritorio o `selector` | Todo lo del usuario, y ceder sus ficheros a cualquier programa suyo | Otros usuarios, el sistema |
| Un servicio corriente (sonido, red) | Lo que pasa por él, de todos los usuarios: el sonido, o el tráfico sin cifrar | Ficheros, llaves de otros servicios |
| El lector de un disco ajeno | Lo que hay en ese disco | Los demás volúmenes |
| `guardian` | Las contraseñas que se tecleen desde entonces. Contestar "sí" a permisos que un paquete declara | Permisos no declarados. Acciones de administrador sin una contraseña buena: la comprueba `sesiones` |
| El compositor | Todo lo que se ve y se teclea, también las contraseñas | Ficheros que no estén en pantalla |
| El gestor de paquetes | Escribir en `Paquetes:`: código que luego ejecutan todos los usuarios | Los cajones cifrados, hasta que su dueño entre |
| `archivos` | Todos los volúmenes abiertos y sus claves | Los cajones cifrados de quien no tiene sesión |
| `sesiones` | Todo, salvo los cajones cifrados cerrados | — |
| Un controlador con acceso directo a memoria (USB, GPU, red, tarjeta) | En la Pi 4, lo mismo que el kernel: no hay IOMMU (S9 de `REVISION_ARQUITECTURA.md`) | — |
| `inicio`, el kernel | Todo | — |

### Reglas para quien implemente

1. **Un servicio actúa con la ficha del cliente, nunca con la suya.** Ninguna orden hace por el cliente algo que su
   ficha no permite. Es la defensa contra que lo usen de intermediario.
2. **Las llaves de permisos se dan sin el derecho de transferir.** Una aplicación con red no puede regalar su llave a
   otra. Hay que comprobar antes que ningún servicio necesita que el cliente pase su llave.
3. **Un enlace nunca lleva fuera de la raíz** por la que se llegó. Igual que "cajón padre".
4. **Los discos ajenos se leen en un proceso aparte**: FAT, los del Amiga, ext4, ADF y HDF. Ese lector solo tiene la
   llave del aparato y un canal con `archivos`. Hoy los lee `archivos`, que lo ve todo: un pendrive hecho a mala idea
   que rompa el lector lo gana todo.
5. **Quien lee datos de fuera no tiene poderes.** Imágenes, tipos de letra, sonidos y páginas web se decodifican en
   el proceso de la aplicación. Nunca en el compositor, `sesiones`, `guardian` ni `archivos`.
6. **Las aplicaciones encerradas no cargan bibliotecas de `Home:Libs`.** Solo de su paquete, de `Paquetes:` y de
   `SYS:`. Si no, un programa del Shell deja una biblioteca en `Home:Libs` y se cuela en todas las aplicaciones del
   usuario, con sus permisos. Cambia S4b para ese perfil. La única excepción es la aplicación que el usuario marca
   en modo de desarrollo, con la confirmación de `guardian` (ver "Perfiles").
7. **Ningún mensaje tumba un servicio.** Cada campo que manda un cliente se valida: tamaños, índices, asas. Cada
   servicio tiene su prueba de mensajes al azar.
8. **Lo que cae se relanza con sus mismas llaves, no con más**, y queda apuntado en el registro de seguridad.
9. **Las contraseñas se borran de la memoria** al acabar de usarlas, y el kernel pone a cero los bloques de mensaje.

### Pruebas hostiles

Cada camino tiene un programa que lo intenta y tiene que fallar. Van en `make prueba`, en la fase que toca:

| Intento | Fase |
|---|---|
| Leer un fichero de otro usuario; subir por encima de la raíz con "padre" y con un enlace | U1 |
| Ceder una asa sin el derecho de repartir; ceder con más derechos de los que se tienen | U1 |
| Mandar a un servicio un alta falsa (solo vale sin marca) | U1 |
| Matar un proceso de otro usuario | U2 |
| Entrar con contraseña mala; leer el volumen con la sesión cerrada | U3 |
| Pasar a otro proceso una llave de permiso; usarla después de revocada | U4 |
| Una acción de administrador sin contraseña; quitar el teclado a `guardian` | U5 |
| Mensajes al azar a `sesiones` y a `archivos` desde un cliente sin permisos | U1 en adelante |

### Lo que este diseño no arregla

- Un fallo del kernel lo da todo. La Pi 4 no tiene defensas por hardware (S16).
- Un controlador con acceso directo a memoria vale como el kernel en la Pi 4.
- La ejecución especulativa: las defensas de S16 están sin medir.
- Dos aplicaciones que se ponen de acuerdo: una con red puede hacer de recadera de otra sin red.

## Decidido el 2026-11-15: identidad de la aplicación y paquetes sueltos (con S8)

- La identidad de una aplicación para lo concedido es repositorio + nombre del paquete, no solo el nombre. Si cambia
  de origen, lo concedido se pierde.
- Un paquete suelto (de un USB) tiene identidad propia aunque vaya firmado con una clave conocida: no hereda lo
  concedido al del repositorio con su mismo nombre. Sin firma válida es la ficha "suelto" de la tabla.
- Los nombres de permisos del formato de paquetes son los de la lista de permisos de este documento, en inglés:
  `network`, `network-listen`, `camera`, `microphone`, `location`, `files`, `volumes`, `deadline`, `screen`,
  `arexx:PORT`, `autostart`. `files:RUTA` no existe.

## Lo que cambia en cada pieza

| Pieza | Cambio |
|---|---|
| kernel | Aviso de muerte de un proceso por canal (K7 de `REVISION_ARQUITECTURA.md`). La clase de plazo pasa a pedir una llave. Poner a cero los bloques de mensaje al soltarlos. Nada más |
| `inicio` | Lanza `sesiones` y le pasa los extremos sin marca de los servicios. Deja de marcar clientes |
| `sesiones` | Nuevo |
| `guardian`, `selector` | Nuevos. Necesitan un papel propio en el compositor |
| `archivos` | Alta con usuario, grupos y derechos. Capa 1 y capa 2. Órdenes nuevas: ceder, cambiar dueño, comprobar contraseña. Clientes sin el tope de 8, con la ventana al primer uso. Los enlaces no salen de la raíz. Los discos ajenos, en un proceso lector aparte |
| compositor, sonido, red | Alta ampliada. Derechos por cliente: capturar, micrófono, escuchar |
| biblioteca (`usuario/`, `kit/`) | Canal de sesión, tabla de nombres, pedir servicios. La de Dash, por la sesión de Dash |
| S8 | Los datos del paquete declaran permisos con los nombres de la tabla |
| S4b | Volumen `Estado:` |

## Fases

Cada una acaba con pruebas en `make prueba` y deja el sistema arrancando.

| Fase | Qué | Prueba que tiene que pasar |
|---|---|---|
| U0 | Condiciones previas: azar de calidad, aviso de muerte, vigilancia de servicios | — |
| U1 | `archivos`: alta ampliada, capa 1, capa 2 y ceder. `inicio` hace de repartidor provisional | Un programa hostil no lee un fichero de otro usuario ni sale de su raíz. Es la prueba de S0: "una aplicación sin permiso no lee `HOME`" |
| U2 | `sesiones`: cuentas, fichas, lanzar desde disco, tabla de nombres, procesos y matar | Dos programas de dos usuarios; uno no mata al otro |
| U3 | Entrada, bloqueo y salida, con el volumen cifrado por usuario | Con la sesión cerrada, el volumen no se lee; contraseña mala, no entra |
| U4 | Permisos por aplicación: declarar, preguntar, guardar, revocar. Llave de plazo | Sin permiso no hay llave; al revocar, el servicio deja de contestar |
| U5 | `guardian` y `selector`: acciones de administrador, selector, doble clic y arrastrar | Un programa solo abre el fichero elegido; una acción sin contraseña no se hace |
| U6 | `Prefs` de usuarios y permisos con sus órdenes (`Usuario`, `Permiso`), registro de seguridad, PID en el kernel, cambio rápido de usuario. El lector de discos ajenos en proceso aparte (se puede adelantar: no depende de lo demás) | Un disco ajeno roto a propósito no tumba `archivos` |

U1 se puede hacer ya: no depende del azar ni del escritorio.

## Decisiones

Fran aprobó las nueve el 2026-11-10:

1. La identidad va en la marca de las llaves; el kernel no aprende qué es un usuario.
2. El usuario 0 no tiene privilegios y el bit `setuid` no hace nada.
3. El Shell es como la caja Amiga (`Home:`, `Work:` y red); un programa suelto con doble clic va encerrado.
4. Ni un administrador lee el cajón de otro usuario; cada usuario recibe un código de recuperación en papel.
5. Cada acción de administrador se confirma, sin plazo de gracia. Confirmado el 2026-11-16 (S36), con un ajuste:
   una tanda es una acción (una ventana con la lista entera y una contraseña).
6. Las ventanas del sistema llevan una imagen o frase secreta del usuario (el detalle, en S10).
7. Volumen `Estado:` para lo que escribe el sistema.
8. Los discos ajenos se leen en un proceso aparte.
9. Las aplicaciones encerradas no cargan de `Home:Libs`. Confirmado el 2026-11-16 (S36), con un ajuste: el modo
   de desarrollo por aplicación y la orden que lanza un programa encerrado desde el Shell (ver "Perfiles").

## Cómo encaja

- Con S1: no añade autoridad ambiental. Todo sigue saliendo de las llaves.
- Con S2: matar es matar el grupo; la clase de plazo es una llave.
- Con S4 y S7: cada caja es una ficha. Dentro de la de Linux, el usuario de la sesión es el suyo y los demás
  aparecen como "nadie".
- Con S4b: el espacio de nombres de la sesión es la tabla de nombres más las raíces. Añade `Estado:`. Lo concedido
  vive en `Home:Prefs/Permisos/`; el cajón de la caja Amiga es `Home:Amiga`.
- Con S5: usa dueño, grupo y modo, y los volúmenes cifrados con sus ranuras.
- Con S6: lo concreta. Cambia una cosa: la identidad no la guarda el kernel.
- Con S8: los paquetes declaran permisos; el nombre del paquete es la identidad de la aplicación.
- Con S10: papel de `guardian` en el compositor, pantalla de entrada y bloqueo.
- Con S14: `inicio` vigila; los servicios se declaran con su usuario.
- Con S16: depende del azar de calidad. S16 evita los fallos del kernel; este documento acota lo que da uno.

## Lo que no se comprobó

- Cuánta memoria gasta Argon2id (64 MB mientras comprueba) en una Pi con poca RAM.
- Nada de esto se probó. Los números de órdenes nuevas se fijan al implementar, en `servicios.md`.
