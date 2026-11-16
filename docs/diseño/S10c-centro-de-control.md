# S10c. El centro de control

Lo pidió el usuario el 2026-11-12: "Para configurar el sistema y el escritorio estaría bien tener un centro de control
tan potente y bien pensado como el de DMS (dank material shell)".

DMS (DankMaterialShell, hoy Dux Modular Shell, `github.com/duxlabs-dev/dux-shell`, licencia MIT) es un escritorio
para niri y Hyprland hecho con Quickshell. De DMS se toman **ideas**, no código: DashOS no usa QML ni Quickshell, y
todo se hace con el kit (Material 3) sobre los servicios de DashOS.

## Lo que se toma de DMS

- **Dos piezas.** Un panel rápido que sale de la barra (lo de cada momento: volumen, brillo, red, modo noche) y una
  ventana de Ajustes con todo lo demás, por categorías en una barra lateral.
- **Enlaces directos.** Cada categoría tiene nombre (`sonido`, `pantallas`, `permisos`...) y se abre directamente
  desde cualquier sitio: un clic en el reloj abre la de fecha y hora; uno en el icono del sonido, la de sonido. En DMS
  es `dms ipc call settings openWith <pestaña>`.
- **Aparatos fijados.** El brillo se lleva por aparato (pantalla interna, monitor externo por DDC, por software); los
  fijados se mueven juntos con el control del panel. Lo mismo para salidas de sonido.
- **Por pantalla.** El panel se abre en la pantalla donde está el ratón, y lo fijado es de cada pantalla.
- **Colores del fondo.** El tema sale del fondo de escritorio (en DMS, con matugen: color dominante, el más vivo, el
  más oscuro...), con claro, oscuro o automático, y se lleva a los programas (en DashOS: el kit, el puente de Linux y
  las pantallas del Amiga, S23).
- **Una categoría por cada cosa del sistema.** La lista de DMS: fondo y colores, estilo, letras y movimiento, barra,
  dock, lanzador, escritorio, notificaciones, avisos en pantalla, bienestar digital, sonido, pantallas, entrada,
  energía, red, usuarios, seguridad, aplicaciones, fecha y región, sistema, complementos, acerca de.

## Lo que se hace distinto (por seguridad, S6b)

- **Ajustes es un programa del sistema**, como `guardian` y `selector`: lo lanza `inicio` con ese nombre, y nadie más
  puede llamarse así.
- **Los ajustes del usuario se cambian también desde fuera, por su fichero** (corregido por el repaso de S0, S36,
  regla 1; antes decía "nadie cambia un ajuste desde fuera"). Cada ajuste es un fichero de texto en `ENVARC:` que
  manda (S4b, S27 apartado 7). Lo escriben la ventana, el editor suelto de `SYS:Prefs` y la orden `Ajuste`; editarlo
  a mano vale igual. Quien puede escribir ese fichero es su dueño y los programas a los que él dio ese alcance (el
  Shell; una aplicación encerrada no ve `ENVARC:` de otros, S6b). El canal "ajustes" sigue sirviendo solo para
  **abrir** una categoría: no hace falta un `settings set` por socket, porque el fichero ya es la interfaz.
- **Protegido queda solo lo de seguridad**: permisos, cuentas y ajustes de la máquina (`Estado:Prefs`, S4b). Eso pasa
  por `guardian`, venga de la ventana o del Shell: ni Ajustes ni una orden lo cambian por su cuenta.
- **Los permisos se leen** (S36, regla 3). Lo concedido está en un solo sitio, `Home:Prefs/Permisos/` (S6b), en texto
  que su dueño lee y solo `sesiones` escribe. Ver los propios es libre: en Ajustes y con la orden `Permiso`. Dar o
  quitar uno, también desde el Shell, pide la confirmación de `guardian`.
- **Las acciones de administrador piden contraseña** (U5, `ADMINISTRAR`): `guardian` la pide, no Ajustes.
- **Los complementos de terceros** (sus propias categorías) van con los permisos de su paquete (S8), en su proceso, y
  dibujan dentro de su hueco de la ventana.

## Fases

| Fase | Qué | Prueba |
|---|---|---|
| C1 | Ventana de Ajustes con barra lateral y búsqueda. Primera categoría, **Permisos** (Prefs de U6): lo concedido a cada programa, y quitarlo (se revoca en el acto). Después **Seguridad** (el registro de `sesiones`, que es el de `Estado:Registros/` y se lee también con la orden `Registro`, S6) y **Discos** (comprobar, instantáneas, volúmenes; lo mismo que la orden `Instantanea`, S5) | Un programa encerrado no puede listar los permisos de otros ni dar o quitar ninguno sin la confirmación de `guardian`; el usuario lista los suyos con `Permiso`; quitar uno revoca la llave del programa abierto. (Hoy la prueba hecha es la de antes: solo Ajustes lista y quita; se cambia al hacer la orden) |
| C2 | Panel rápido desde la barra: volumen, brillo, modo noche, no molestar, bloquear, salir. Mosaicos que se añaden, quitan y ordenan (modo de edición) | El panel no se lleva el teclado de otra ventana sin un clic (S8) |
| C3 | Apariencia: claro, oscuro o automático; colores sacados del fondo (esquema de Material 3 hecho aquí, desde la especificación); letras; movimiento | Los colores salen con contraste suficiente en claro y oscuro |
| C4 | Sonido y pantallas con aparatos fijados; entrada (teclado, ratón, distribución) | |
| C5 | Red, Bluetooth y energía, cuando existan sus servicios (S12, S13, S22) | |
| C6 | Complementos: categorías de terceros, con los permisos de su paquete (S8) | Un complemento no ve las otras categorías ni los permisos |

Cada fase, con capturas al usuario.

## Fuentes

- README de Dux Modular Shell: https://github.com/duxlabs-dev/dux-shell
- Documentación: https://duxlabs.dev/docs/dms/overview, https://duxlabs.dev/docs/dms/keybinds-ipc (la lista de
  categorías de Ajustes y los enlaces directos), https://duxlabs.dev/docs/dms/application-themes (colores del fondo).
- No documentado en lo leído: la lista exacta de mosaicos del panel rápido y su modo de edición; eso se diseña aquí.
