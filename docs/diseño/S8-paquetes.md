# S8. Paquetes y Hangar

## El problema

El encargo pide paquetes que se actualicen como con pacman, y que Hangar forme parte. Y las tareas anteriores fijan
el terreno:

- **`SYS:` es de solo lectura** y solo lo cambian las actualizaciones firmadas (S4b, S6).
- **Lo instalado va a `Paquetes:`**, un cajón por paquete, y cada paquete declara sus asignaciones en vez de tocar
  `S:User-Startup` (S4b).
- **Cada usuario puede instalar en su `Home:`** sin ser administrador (S4b).
- **Las instantáneas** del sistema de archivos hacen posible que todo sea atómico y reversible (S5).
- **Firmas Ed25519** para paquetes y repositorios (S6).
- **Cada paquete declara los permisos que necesita** (S6).
- **Hay tres mundos de programas**: nativos de DashOS, Amiga 68k y Linux (S4, S7).

## Qué es Hangar hoy

Hangar es el catálogo y gestor de programas para AmigaOS 3.x, en desarrollo. Tiene dos
piezas: un cliente para el Amiga, escrito en Dash con MUI, y un servicio en internet, en Rust con PostgreSQL. El
servicio recoge el catálogo de Aminet y otras fuentes, con fichas e imágenes. El cliente busca, instala con recibo,
actualiza, desinstala y ejecuta. Los programas se bajan de su sitio original.

## Lo que aprendieron otros

- **Fedora Silverblue (OSTree y rpm-ostree)** separa el sistema base, que es una imagen versionada y de solo
  lectura, de los paquetes que el usuario añade encima. Las actualizaciones se preparan en segundo plano y entran al
  reiniciar, y se puede volver a la versión anterior sin tocar los datos del usuario. Fedora recomienda Flatpak para
  las aplicaciones gráficas y deja las capas de paquetes para lo del sistema.
- **casync** parte una imagen en trozos por contenido. Al actualizar, solo se bajan los trozos nuevos.
- **pacman** es sencillo y rápido: una base de datos de paquetes por repositorio, dependencias, ganchos tras
  instalar y una orden para ponerlo todo al día (`pacman -Syu`).
- **amipkg**, el gestor de paquetes para AmigaOS 3.x que Hangar estudió, guarda un recibo con cada fichero instalado
  y firma su catálogo con Ed25519.

## Recomendación

### Dos niveles, como Silverblue

1. **El sistema base (`SYS:`) es una imagen**, no una colección de paquetes sueltos. Se construye en el servidor a
   partir de paquetes, se firma entera, y llega como una instantánea nueva de `SYS:` (S5). Se aplica al reiniciar. Si
   el arranque falla, se vuelve a la anterior (S14).
2. **Lo demás son paquetes**, en `Paquetes:` (de todo el sistema) o en `Home:` (de un usuario). Cada instalación,
   actualización o desinstalación es una transacción: se escribe una generación nueva y solo si todo sale bien se
   cambia a ella (ver "Generaciones"; antes se pensó como una instantánea nueva de `Paquetes:`). Si algo falla, no
   ha pasado nada.

Para el usuario hay una sola orden, como en pacman, que pone al día las dos cosas.

### El gestor: `Paq`

Nombre provisional. Es una orden en `C:` y un servicio del sistema:

- `Paq Instalar <nombre>`, `Paq Quitar <nombre>`, `Paq Buscar <texto>`, `Paq Actualizar` (todo al día) y
  `Paq Volver` (deshacer la última transacción). Con alias cortos en inglés para quien venga de Linux.
- Todo lo que Ajustes hace con los paquetes tiene su orden (S0): `Paq Generaciones` (la lista, con fecha y lo que
  cambió), `Paq Volver <n>` (a una de la lista), `Paq Limpiar` (lo mismo que "limpiar" en Ajustes) y
  `Paq Info <nombre>` (versión, permisos que pide y ficheros).
- **Repositorios** con un índice firmado. Claves de confianza: la de DashOS, la de Hangar y las que añada un
  administrador (S6).
- **La lista de repositorios y las claves de confianza son ficheros de texto** en `Estado:`, que todos leen: la
  lista, en uno; las claves, un fichero por clave. Se miran y se cambian con `Paq Repos` y `Paq Claves`, y en
  Ajustes. Añadir un repositorio o una clave sigue siendo acción de administrador (S6b). Cajón y formato, por diseñar.
- **Dependencias** por nombre y versión, y por lo que un paquete ofrece, por ejemplo `mui.library 20`.
- **Sin choques de ficheros**: cada paquete vive en su cajón, y las asignaciones en capas (S4b) deciden quién gana.
  Si dos paquetes ofrecen la misma biblioteca, no se pregunta: manda la prioridad que diga el repositorio. Quien
  quiera otra la elige con una orden de `Paq`.
- **Actualizaciones por trozos**, como casync, para la imagen de `SYS:` y para los paquetes grandes.
- **Recetas**: cada paquete nativo se construye desde una receta en un repositorio, al estilo de los `PKGBUILD` de
  Arch, en un servidor de construcción.

### Qué lleva un paquete

- Su cajón: `C`, `Libs`, `Devs`, `Fonts`, `Locale`, `Prefs`, iconos y el programa.
- Sus datos: nombre, versión, arquitectura, dependencias, asignaciones que necesita (S4b) y permisos que pide (S6).
- Su firma.

### Los tres mundos

| Clase | De dónde sale | Dónde se instala | Quién lo gestiona |
|---|---|---|---|
| Nativo de DashOS | Repositorio de DashOS y Hangar | `Paquetes:` o `Home:` | `Paq` |
| Amiga 68k | Hangar (Aminet, GitHub, autores) | `Paquetes:Amiga/<programa>` o donde el usuario elija | `Paq`, con el recibo de Hangar |
| Linux | La distribución de la caja Linux (S7) | Dentro de la caja Linux | El gestor de la distribución, por ejemplo pacman de Arch Linux ARM |

- **Programas Amiga**: muchos traen un guion de `Installer`. Se ejecuta dentro de la caja, con el `Installer` del
  Amiga, y `Paq` apunta lo que escribió, como el recibo de Hangar. Si no hay guion, se copia el cajón entero.
- **Programas de Linux**: los instala la distribución de dentro, con su propio gestor. DashOS solo pone sus iconos en
  el lanzador (S7).

### Hangar en DashOS

**Hangar es el catálogo y `Paq` es el motor.** Hangar no instala por su cuenta: le pide a `Paq` que lo haga.

- **El escaparate**: fichas con imágenes, búsqueda, novedades y valoraciones. Lo que Hangar ya hace.
- **Un solo escaparate, tres mundos** (decidido por el usuario, 2026-11-10): nativos de DashOS (`Paq`), programas
  Amiga y los de la caja Linux (Flatpak para las aplicaciones de escritorio y pacman para lo demás), juntos en las
  mismas búsquedas y fichas. Por debajo, cada mundo con su gestor: Hangar le pide a `Paq`, a Flatpak o a pacman que
  instale, y enseña el resultado. Quien quiera usa `pacman` o `flatpak` a mano en la terminal de la caja.
- **El cliente de Hangar que hoy es para el Amiga ya funcionaría en DashOS el primer día**, dentro de la caja Amiga:
  es un programa 68k con MUI. Cuando el backend ARM de Dash exista, el mismo código se compila en nativo.

Hangar tendría que ganar una arquitectura nueva en su catálogo, la de DashOS, al lado de la del 68k. Es una decisión
del proyecto Hangar, que tiene abierta la pregunta de qué sistemas cubre. Este bucle no toca Hangar.

### Actualizaciones

- `Paq` mira si hay novedades una vez al día, y baja en segundo plano.
- Los paquetes se actualizan al momento. Un programa abierto sigue con su versión hasta que se cierra (ver
  "Generaciones"), y la nueva entra la próxima vez que se abre, sin aviso. Solo se avisa para reiniciarlo si la
  actualización es de seguridad.
- `SYS:` se actualiza al reiniciar. Se avisa, nunca se fuerza.
- Las actualizaciones de seguridad se marcan como tales en el índice.

## Decidido el 2026-11-15 (formato de paquetes 2, lo que quedaba abierto)

**La máquina de un paquete Amiga** es la caja, con el perfil que ella dice (S4).

**Vocabulario de máquina de `aarch64-dashos`.** Mínimo de procesador: ARMv8.0-A. Lo que un programa puede mirar al
arrancar (cifrado, LSE y demás extensiones) no se declara: lo detecta él y elige su camino. Todo binario de DashOS se
enlaza para páginas de 16 KB, que valen también con 4 KB: así no hay paquetes aparte para el Mac. Se declara solo lo
que no se arregla al arrancar: `hypervisor` (hay EL2 para cajas) y la memoria mínima (`4GB`). Lo demás se añade
cuando un programa de verdad lo pida; una palabra que un `Paq` viejo no conoce es un error, y no instala.

**Instaladores Amiga (`installer`).** No se le prohíbe escribir fuera de su cajón (casi todos copian a `LIBS:` y tocan
`S:User-Startup`, y abortarían): corre en una caja de instalación aparte, sin red y con tope de tiempo, sobre una
capa de captura que no toca el sistema. Al acabar, `Paq` reparte lo capturado: lo del cajón elegido, al cajón del
paquete; lo de `C:`, `LIBS:`, `DEVS:`, `FONTS:` y `LOCALE:`, a las capas del paquete (la lista cerrada de S4b); de
`S:User-Startup` solo las líneas `Assign`, que pasan a ser sus asignaciones; lo demás se descarta y se enseña, y la
lista de lo descartado se guarda en un fichero de texto junto al paquete. Hace falta el `Installer` del AmigaOS del
usuario (DashOS no lo trae): sin él, el paquete se instala como cajón, y se dice en llano qué falta y dónde ponerlo
("Este programa trae un instalador de Amiga y falta el programa Installer. Se ha copiado tal cual").

**Firmas.** Lo que baja de un repositorio va comprobado por el índice firmado, que lleva la huella de cada paquete.
La firma del fichero (`PKG-FILE-2`) es opcional y sirve para lo suelto (un USB, lo entregado fuera):

- solo cuenta si la clave está en la lista del usuario; con una clave desconocida es como sin firma;
- un paquete suelto firmado no lleva serie ni caducidad: nunca sustituye a una versión más nueva sin preguntar;
- lo suelto tiene identidad propia (no la del repositorio): aunque se llame igual, no hereda lo concedido (S6b);
- sin firma válida es un "suelto" de S6b: se instala solo si el usuario lo confirma, y pregunta todo.

## Decidido el 2026-11-15 (2): el reparto con Hangar

- **Instala `Paq`.** Resuelve, baja, verifica y guarda la base; el cliente de Hangar en DashOS es la cara: enseña y
  pide. El cliente 68k dentro de la caja Amiga instala por su cuenta en `Work:` de la caja, fuera de `Paq`.
- **Dónde escribe `Paq`**: `Paquetes:` para lo de todos y `Home:` para lo del usuario. En DashOS un `drawer` nunca va
  a `Work:`. (S6b:185 le daba solo `Paquetes:`: falta su fila para la instalación de usuario.)
- **El código común del formato** (`pkg2`: versiones, requisitos, manifiesto, índice, firmas, resolutor), con la
  especificación y sus vectores, sale a un repositorio abierto con MPL-2.0; lo usan `Paq`, los clientes y el
  servicio. Los clientes de Hangar son GPL y abiertos; el servicio web es privado.
- Por limpiar en estos diseños (texto de antes del 15/11): "paquetes firmados" (S8, S6, S29, RESUMEN), "la clave de
  Hangar" (S8, S6, S29), identidad solo por nombre (S6b), Installer "dentro de la caja" (S8). La clave de `SYS:` no
  está en la lista de `Paq`: falta decirlo en S6 y S14.

## Del formato de paquetes 2 (2026-11-15): claves raíz y resolutor

- **Claves raíz**: cada repositorio tiene dos raíces fuera de línea que solo firman `<url>/keys` (y `keys.sig`, una
  línea por raíz, etiqueta `PKG-KEYS-2`). `keys` dice qué claves de índice y de respuestas valen (varias al rotar),
  cuáles están anuladas (`Revoked`: lo firmado con ellas deja de valer) y, si hace falta, una raíz nueva firmada por
  una conocida; con `Serial` que solo crece y `Expires` largo. `Paq` trae de fábrica solo las públicas de las raíces
  y mira `keys` al empezar, al caducar y cuando ve una firma de una clave que no conoce. Para el repositorio de
  DashOS, sus dos raíces son de DashOS; la clave que firma la imagen de `SYS:` sigue fuera de la lista de `Paq` (S6).
- **Resolutor**: al instalar no rompe lo instalado (falla diciendo qué rompería o con qué choca); en "actualizar
  todo", lo que rompería a otro se queda donde está y se dice por qué. `Paq` usa `pkg2::plan::comprobar` (todo plan
  pasa por él antes de aplicarse) y `pkg2::resolutor::resolver`, en Rust `no_std`, que el código común abierto
  (MPL-2.0) traerá con sus vectores.

- **Dependencias declaradas o adivinadas** (`Declared: no` en la entrada): las que no escribió el autor (salen de un
  readme de Aminet o las adivinó el repositorio) se enseñan y avisan, pero no bloquean ni cuentan en el comprobador de
  planes. Hoy todo lo de `aminet` es `Declared: no`; lo de `main`, declarado.
- **Volver a una versión anterior**: una respuesta firmada da las anteriores que aún se pueden bajar (hasta diez); el
  plan es instalar esa entrada (puede bajar de versión, solo si se pide). `Paq` guarda en caché el último paquete
  instalado de cada uno, para volver sin red. En DashOS, además, las instantáneas (S5).

- **Espacio antes de empezar**: con `Size` (la bajada) e `Installed-Size` (lo descomprimido), `Paq` suma por volumen
  bajada + descomprimido + copias de lo que pisa, y no empieza si no cabe.
- **La base protegida**: cada `.pkg` de la base acaba en `Sum:` (los 16 primeros del SHA-256 del texto anterior); si
  no cuadra, el paquete queda "en duda" y se rehace desde la copia de la base o se ofrece reinstalar. Antes de cada
  transacción, copia de la base entera. En DashOS, además, la instantánea (S5).

- **Repositorio estático**: una carpeta servida tal cual (`index.gz`, `index.gz.sig`, `keys` y los paquetes), sin
  respuestas firmadas; vale para un gestor que lee el índice entero, como `Paq`. El repositorio de DashOS puede ser
  así, y cualquiera puede montar el suyo (la herramienta de referencia lo genera: solo `Format: 2`, la versión más alta
  de cada nombre, caducidad a 30 días).
- **Un fichero cambiado en su sitio** (mismo nombre y versión, otra huella: Aminet, un adjunto de GitHub resubido)
  entra con el `Release` siguiente: `Paq` lo ve como actualización, nunca como el mismo paquete.
- **Cuándo lee `Paq` el fichero `keys`**: al empezar si no lo tiene, al caducar (con reloj), y cuando ve una firma de
  una clave que no conoce; con `keys` leído, solo valen sus claves no anuladas.

## Generaciones, como en NixOS (decidido 2026-11-15)

Viene de S35 ("Lo que se toma de NixOS"). El usuario lo aceptó con una condición: que sea elegante y que no le dé
trabajo a quien usa el sistema. Esa condición manda sobre todo lo demás de este apartado.

**Qué es.** Cada versión de un paquete se guarda en su propio cajón y no se pisa nunca. Una generación es una tabla
pequeña que dice qué versión de cada paquete está en uso. Instalar, actualizar o quitar es escribir los cajones que
falten y cambiar de tabla de golpe. Volver atrás es usar la tabla anterior.

**Lo que ve el usuario: nada nuevo.**

- Las rutas no llevan versión ni huella. El usuario y los programas ven `Paquetes:Tachyon`, como hasta ahora, y los
  iconos, `PROGDIR:` y los guiones siguen valiendo tras actualizar. Las versiones guardadas viven en un almacén que
  solo toca `Paq`, que no estorba en el explorador pero se ve con "Mostrar todos los ficheros" y se lee desde el
  Shell (ordenado, no escondido: S0); `Paquetes:` es la vista de la generación en uso (cada proceso ya
  tiene su vista de los nombres, S4b). La tabla de cada generación es un fichero de texto que se puede leer.
- No hay fichero de configuración que escribir ni lenguaje que aprender. Se instala y se quita igual que antes.
- Una sola acción nueva: "Deshacer la última actualización", en Ajustes y en `Paq`. Quien quiera, puede ver la lista
  de generaciones con su fecha y lo que cambió, y elegir una. Nadie está obligado a mirarla.
- La limpieza es sola: se guardan las tres últimas generaciones y las de los últimos 30 días; lo que ninguna usa se
  borra. Ajustes dice cuánto ocupa y deja limpiar a mano. Si el disco se llena, se limpia antes de fallar.
- Los avisos hablan en llano: "Se actualizó Tachyon. Deshacer", nunca "generación 14".

**Reglas.**

- **Una versión de cada biblioteca, salvo que haga falta otra.** Dos versiones a la vez solo si un paquete lo pide en
  su ficha; entonces ese programa ve la suya y los demás no se enteran. No es lo normal ni se anima a ello.
- **Dos niveles**, como las capas de S4b: las generaciones del sistema (`Paquetes:`, las cambia un administrador) y
  las de cada usuario (`Home:Paquetes`). Deshacer lo de un usuario no toca lo de otro.
- **Los datos no vuelven atrás.** `Home:`, `ENVARC:` y `Estado:` no forman parte de la generación: deshacer una
  actualización no borra documentos ni ajustes. Límite que se dice: si la versión nueva cambió el formato de sus
  ajustes, la vieja puede no entenderlos; el paquete puede declarar "no se puede volver atrás desde esta versión" y
  `Paq` avisa antes de actualizar.
- **El cambio de tabla es atómico**: un corte de luz deja la tabla vieja o la nueva, nunca media (lo da DashFS).
- **Un programa abierto sigue con su versión** hasta que se cierra: su vista no cambia en marcha.
- **Con `SYS:`** no cambia nada: la base sigue siendo una imagen firmada con dos ranuras (S14). Una generación apunta
  la versión de `SYS:` con la que se hizo, para que al arrancar con la ranura anterior se use la tabla que le toca.
- **Con una imagen de hibernación** (S22): si la hay, se arranca con la generación y el `SYS:` que la hicieron, para
  no perder la sesión abierta. La actualización entra en el siguiente arranque limpio.
- **No cambia el formato de paquetes** ni Hangar. Sustituye a la idea anterior de "una instantánea nueva de `Paquetes:` por transacción"
  (punto 2 de arriba, ya cambiado): la transacción es ahora el cambio de tabla. Las instantáneas (S5) siguen para los datos.

**Lo que no se toma de NixOS**: su lenguaje, las huellas en las rutas y montar el sistema entero a base de enlaces.

**Coste que se acepta**: más disco mientras se guardan versiones viejas (lo acota la limpieza), y que `Paq` lleve la
tabla. **Por diseñar al hacerlo**: el formato de la tabla (texto), dónde vive el almacén, y cómo se resuelven
`Paquetes:*/C` y las demás capas con la tabla sin que cueste al abrir programas (hay que medirlo).

**Pruebas hostiles que tendrá**: corte de luz a mitad de actualizar; deshacer con un programa abierto; disco lleno;
un programa que intenta escribir en el almacén o en la tabla; una tabla dañada (se usa la anterior y se avisa).

## Lo que queda abierto

1. ¿Nombre de la orden: `Paq`, u otro?
2. ¿Sistema base como imagen firmada, y paquetes solo por encima, como Silverblue? Recomendado: sí. La otra opción es
   que todo sean paquetes, como Arch, que es más flexible pero menos seguro de actualizar.
3. ¿Hangar como catálogo encima de `Paq`, con una arquitectura nueva de DashOS en su catálogo? Recomendado:
   sí. Toca decidirlo en el proyecto Hangar.

## Cómo encaja

- Con S0: principio 3 (todo o nada) y principio 4 (un solo sistema).
- Con S4: los programas Amiga se instalan con su `Installer`, dentro de la caja.
- Con S4b: `Paquetes:`, capas de asignaciones, y nada de `S:User-Startup`.
- Con S5: transacciones e imagen de `SYS:` como instantáneas.
- Con S6: firmas Ed25519 y permisos declarados por paquete.
- Con S7: lo de Linux lo gestiona la distribución de la caja Linux.
- S14: arranque con la instantánea nueva de `SYS:` y vuelta atrás si falla.

## Fuentes

- Silverblue y rpm-ostree: [documentación de rpm-ostree](https://coreos.github.io/rpm-ostree/),
  [repositorio](https://github.com/jlebon/rpm-ostree) y
  [experiencias en Fedora](https://discussion.fedoraproject.org/t/experiences-with-package-layering/1186).
- casync: [paquete de Fedora](https://packages.fedoraproject.org/pkgs/casync/casync) y
  [artículo sobre troceado por contenido](https://ranger.uta.edu/~jiang/publication/Conferences/2020/MSST20-DSync(WenXia).pdf).
- Hangar y amipkg: el plan del proyecto Hangar (privado), solo leído.
- pacman: funcionamiento conocido.

No se comprobó nada en código: es diseño.
