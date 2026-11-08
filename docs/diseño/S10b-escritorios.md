# S10b. Escritorios: un compositor y escritorios de quien quiera

Del 2026-11-08. Lo pide el usuario. Es análisis y diseño: no hay código.

## El problema

El usuario, con sus palabras: "Voy buscando una arquitectura formada por un compositor, y otra en la que la gente
pueda construir sus propios escritorios. Algo parecido a lo que se hace con Niri y Noctalia y DMS."

Son dos piezas:

- **Un compositor oficial.** Es el único proceso que tiene la pantalla, la GPU de presentación y el teclado. Coloca
  las ventanas. Tiene que ser de confianza.
- **Escritorios** (lo que en Linux se llama "shell"): la barra, el dock, el lanzador, las notificaciones, el fondo,
  los avisos de volumen y brillo, el centro de control. Los puede escribir cualquiera. Son procesos sin privilegios,
  con poderes concretos que se pueden quitar.

Más adelante el usuario pasará un vídeo con la idea del escritorio oficial de DashOS. Ese escritorio será uno más:
usa la misma interfaz que los de terceros.

### Qué cambia respecto a S10

S10 ya separaba el compositor del escritorio y pedía un escritorio al nivel de DMS, sin Quickshell. Este documento
mantiene eso y cambia cinco cosas:

1. **La frontera entre los dos pasa a ser pública y estable.** En S10 era una interfaz interna. Ahora es un protocolo
   documentado que usa cualquier escritorio, también el oficial.
2. **El estado sale del escritorio.** Notificaciones, bandeja, música, red, sonido y apariencia son servicios del
   sistema. El escritorio solo los enseña. Cambiar de escritorio no pierde el historial de avisos ni los ajustes.
3. **El bloqueo se parte.** El compositor sigue siendo el dueño del bloqueo, como decía S10, y además dibuja el campo
   de la contraseña. El escritorio puede poner la cara (reloj, fondo, avisos), pero nunca ve las teclas.
4. **Workbench deja de ser parte del escritorio.** Es un programa aparte, como `LoadWB` en el Amiga, con una interfaz
   pública (AppIcons, AppWindows, AppMenus) que otro puede sustituir, como hacían Directory Opus Magellan o Scalos.
5. **Los módulos de terceros son procesos aparte** que dan datos y una descripción de lo que enseñan, no píxeles. S10
   los quería "en Dash, con capacidades justas"; aquí se dice cómo.

La política de ventanas (mosaico desplazable, flotantes, pantallas a lo Amiga) sigue en el compositor, como en S10.

## Lo que se comprobó

En la web (fuentes al final; consultadas el 2026-11-08):

- **niri no es un escritorio completo**, lo dice su README. No trae barra, lanzador, notificaciones, fondo ni bloqueo.
  Su IPC es un socket con JSON. Tiene un flujo de eventos que manda primero el estado entero y después los cambios.
  Su propia documentación avisa de que los cambios no son atómicos: una ventana puede apuntar a un escritorio virtual
  ya borrado.
- **niri confía en todo programa que corre fuera de una caja.** Cualquier cliente puede grabar la pantalla, imitar el
  teclado, leer el portapapeles o tapar la pantalla con una capa. Solo los programas en caja (Flatpak) reciben un
  socket filtrado, con `wp-security-context-v1`. Si el programa que bloquea la sesión se cae, la pantalla se queda
  bloqueada en rojo hasta que otro tome el relevo.
- **Noctalia dejó Quickshell.** Su versión 4 (QML) ya no se mantiene. La 5 (abril de 2026) es C++ sin Qt. Motivos
  que dan: unos 300 MB de memoria por monitor en la 4, frente a la sexta parte en la 5; recompilar con cada versión
  de Qt; el coste del motor de JavaScript. Lleva el código de cada compositor (Hyprland, niri, Sway, mango) en una
  capa aparte. Sus complementos van en Luau y los da por fiables: instalar uno es como ejecutar un guion tuyo.
- **El equipo de Noctalia hace ahora su propio compositor, Umbriel.** Esconde a los clientes en caja la captura, la
  entrada falsa, las capas, el bloqueo y el control de monitores y escritorios. Los protocolos nuevos quedan ocultos
  hasta que alguien los revisa. Los permisos por aplicación solo suman.
- **DMS** (ahora "Dux Modular Shell") son dos procesos: un programa en Go, `dms`, que habla con Wayland, D-Bus,
  la red, Bluetooth, el brillo y los complementos; y la interfaz en QML sobre Quickshell, que dibuja. Hablan por un
  socket con JSON. DMS lleva mucho código distinto según el compositor y su versión (Hyprland 0.55 o más, opciones de
  niri que solo enseña si el niri instalado las acepta).
- **Quickshell** sigue en alfa (0.3.2) y avisa de que habrá cambios que rompen. Ya los hubo en la 0.2 y la 0.3.
- **GNOME**: las extensiones son parches del interior de la Shell, que va en el mismo proceso que el compositor. No
  puede haber API estable, y en el peor caso tumban el escritorio. GNOME 45 rompió todas las anteriores.
- **Hyprland**: los complementos son bibliotecas de C++ cargadas en el compositor. Tras cada actualización hay que
  recompilarlos, y si no, el compositor no arranca.
- **KDE**: el escritorio (plasmashell) es un cliente aparte de KWin. KWin daba los protocolos privilegiados según un
  campo del fichero `.desktop` del programa; en 2019 se vio que un `.desktop` en la carpeta del usuario se lo saltaba.
- **COSMIC**: el panel es un proceso; cada applet es otro proceso con su propia ventana, que el panel coloca dentro
  de su capa haciendo de compositor pequeño.
- **river 0.4** (marzo de 2026) saca la política de ventanas a otro proceso con un protocolo estable. El gestor
  decide posición, tamaño, atajos, foco y bordes. El compositor abre transacciones atómicas con un plazo corto, y no
  hay ida y vuelta por cada cuadro ni por cada tecla. Si el gestor se cae, la sesión sigue, y se puede cambiar de
  gestor en caliente.
- **Amiga**: los sustitutos de Workbench parcheaban los vectores de `workbench.library`. Las notas de la V44 ya decían
  que sería mejor sustituir la biblioteca entera.

En el repositorio:

- **El compositor de hoy es provisional** (`usuario/src/bin/compositor.rs`, C50): pinta un fondo y una ventana que
  se mueve con el ratón. No habla Wayland. Funciona en la Pi (buzón de la VideoCore) y en QEMU `virt` (pantalla de
  UEFI con `ramfb`), con teclado y ratón por el canal "entrada".
- **Ya existe el patrón de "poderes por cliente".** Cada canal de `inicio` lleva la marca de quien habla, y el buzón
  deja a cada cliente pedir solo sus etiquetas (`docs/referencia/servicios.md`). Es la misma idea que necesita el
  compositor con los escritorios.

No se comprobó: nada se ha compilado ni medido. Las cifras de memoria de Noctalia son suyas. Cómo empotra COSMIC sus
applets se sabe por su README y por un mensaje en Mastodon, sin leer el código. Si niri tiene hoy
`ext-image-copy-capture` no está claro: su documentación lo nombra y la tabla de wayland.app dice que no.

## 1. Lo que enseñan los demás

| Modelo | Dónde va el escritorio | Qué sale bien | Qué sale mal |
|---|---|---|---|
| GNOME | Dentro del compositor, con extensiones que parchean | Todo integrado | Rompe en cada versión; una extensión tumba la sesión |
| Hyprland | Complementos de C++ dentro del compositor | Pueden cambiarlo todo | Recompilar tras cada versión; un fallo lo tumba |
| KDE | Cliente aparte, con scripts dentro del compositor | Escritorio que se reinicia solo | Permisos por un fichero que el usuario puede escribir |
| niri + DMS o Noctalia | Cliente aparte, por Wayland y por el IPC del compositor | Escritorios de terceros, cambiables | Sin permisos; código por compositor y versión; Qt pesado |
| COSMIC | Panel aparte; cada applet, otro proceso | Un applet que falla no tumba el panel | El panel tiene que hacer de compositor pequeño |
| river 0.4 | Hasta la política de ventanas, fuera | Un gestor que falla no tumba la sesión | Protocolo más complejo; todo nuevo |
| Amiga | Workbench, tarea aparte sobre Intuition | Se cambiaba por DOpus o Scalos | A base de parchear vectores |

Lo que se copia:

- **El escritorio fuera del compositor**, como niri y KDE. Es lo que pidió el usuario y lo que S10 ya decía.
- **Un IPC con el estado entero al conectar y después los cambios**, en un formato que solo añade campos (niri).
  Pero con cambios atómicos, que niri no tiene.
- **Permisos por conexión desde el primer día**, con los protocolos privilegiados ocultos por defecto (Umbriel).
- **Bloqueo que falla cerrado**: si cae quien dibuja el bloqueo, la pantalla sigue tapada (niri, Umbriel).
- **Una interfaz con nombre que se sustituye entera**, la lección de `workbench.library`.

Lo que se evita:

- **Código ajeno dentro del compositor** (GNOME, Hyprland).
- **Que cada escritorio lleve código por compositor y por versión** (DMS, Noctalia). En DashOS hay un solo
  compositor y un protocolo versionado.
- **Dar privilegios por metadatos que el usuario puede escribir** (KWin en 2019).
- **Atar los escritorios a un kit pesado** que cambia por debajo (Quickshell y Qt). Es lo que dijo S10, y Noctalia
  acaba de llegar a la misma conclusión.

## 2. El reparto: compositor, servicios y escritorio

Tres capas en lugar de dos. La del medio es la que falta en Linux: allí cada escritorio vuelve a escribir su
servidor de notificaciones, su agente de Bluetooth y su lectura de la batería (DMS lo hace en Go, Noctalia en C++).

### El compositor (de confianza)

Proceso del sistema, con la clase de plazo del planificador (S2), en `SYS:`, firmado (S8).

- **La pantalla y la presentación**: compone, sincronía vertical, planos del HVS, la GPU (S9).
- **La entrada**: teclado, ratón, panel táctil, gestos. Decide qué cliente recibe cada evento.
- **El foco del teclado.** Un escritorio no puede robarlo; solo pedirlo para sus propias superficies.
- **La política de ventanas**: disposiciones, mosaico y flotantes, pantallas, monitores, vista general, animaciones
  de las ventanas (S10). Ver la sección 5.
- **Los bordes de las ventanas** (S4: Amiga por dentro, DashOS por fuera).
- **El bloqueo**: tapa la pantalla, guarda la entrada y dibuja el campo de la contraseña. Ver la sección 3.
- **Los atajos reservados**: el de rescate y los del sistema (sección 6). Nunca llegan a un escritorio ni a una
  aplicación.
- **Las ventanas del sistema que piden confianza**: la confirmación de administrador (S6), la pregunta de permisos
  la primera vez (S6) y el aviso de "se está grabando la pantalla". Si las dibujara el escritorio, un escritorio
  malo podría falsificarlas.
- **Las capturas de pantalla**, solo hacia quien tenga el poder, y con un indicador que el escritorio no puede
  quitar.

### Los servicios del escritorio (sin pantalla)

Procesos del sistema sin interfaz. Guardan el estado y lo comparten con quien tenga permiso:

- **Avisos**: reciben las notificaciones (nativas, de la caja Amiga por su puente, de la caja Linux por la norma de
  freedesktop) y guardan el historial.
- **Bandeja**: los iconos de estado de los programas y sus menús.
- **Música**: qué suena y sus controles, como MPRIS en Linux.
- **Apariencia**: el tema activo, los colores sacados del fondo, el fondo de cada pantalla (sección 7).
- **Los que ya existen o van a existir**: red (S13), sonido, energía y batería, Bluetooth, monitores (S9).
- **Workbench** como servicio: AppIcons, AppWindows y AppMenus. Ver la sección 5.

### El escritorio (sin privilegios)

Uno o varios procesos del usuario, con los poderes que el usuario le dio:

- Barra (con los menús del programa activo al botón derecho, S10), dock, lanzador, centro de control, centro de
  avisos, avisos en pantalla de volumen y brillo, menú de apagado, calendario, fondo de pantalla, módulos en el
  escritorio, la cara del bloqueo.
- Todo lo que dibuja va en superficies propias. No toca las ventanas de nadie ni ve lo que tienen dentro.

## 3. Los poderes de un escritorio

Un escritorio es un cliente de Wayland más. Lo que lo distingue son los poderes que el compositor le reconoce en su
conexión. Cada uno se da y se quita por separado, en el panel de poderes de `Prefs` (la dirección aceptada el
2026-11-07: "cajas con pasaporte y panel de poderes revocables").

| Poder | Qué deja hacer | Protocolo | Riesgo si se abusa |
|---|---|---|---|
| Capas | Superficies de fondo, abajo, arriba y encima de todo, con zona reservada (la barra) | `wlr-layer-shell` | Tapar la pantalla para engañar |
| Ver ventanas | Lista de ventanas: título, programa, pantalla, estado | `ext-foreign-toplevel-list` y la extensión propia | Saber qué usa el usuario |
| Mandar ventanas | Activar, cerrar, minimizar, mover de pantalla | Extensión propia | Molestar, no espiar |
| Pantallas | Ver y cambiar de pantalla virtual y de disposición | `ext-workspace` | Poco |
| Menús del programa | Recibir el árbol de menús del programa activo y activar una opción | Extensión propia | Activar opciones de otro programa |
| Atajos | Registrar combinaciones de teclas concretas y recibir solo esas | Extensión propia | Poco: el compositor decide |
| Espejos | Pedir una miniatura en vivo de una ventana, sin recibir sus píxeles | Extensión propia | Ninguno sobre el contenido |
| Cara del bloqueo | Dibujar el fondo del bloqueo | `ext-session-lock`, recortado | Ninguno sobre la contraseña |
| Servicios | Hablar con avisos, bandeja, música, apariencia, red... | Canales de DashOS | Cada servicio decide qué enseña |

Lo que un escritorio **no tiene nunca de serie**, ni el oficial:

- Leer el teclado cuando el foco es de otro.
- Capturar la pantalla o el contenido de una ventana.
- Inyectar teclas o movimientos de ratón.
- Leer el portapapeles sin que el usuario pegue.

Estos cuatro son poderes de aplicación (S6: "actuar sobre otras ventanas"), que se preguntan al usuario y solo
tienen herramientas concretas: la de capturas, la de accesibilidad, el historial del portapapeles.

### Cómo sabe el compositor quién le habla

- **Por la marca del canal, no por lo que diga el cliente.** El transporte de Wayland son canales de DashOS (S9), y
  cada canal lleva la identidad de quien lo abrió (S6). Es lo mismo que ya hace el buzón con sus etiquetas.
- **Los poderes los guarda el sistema**, en un registro que solo cambia `Prefs` tras una confirmación que dibuja el
  compositor. Nunca salen de un fichero que el usuario o un programa puedan escribir (la lección de KWin).
- **Un cliente sin el poder no ve el protocolo.** El compositor no le anuncia ese global. Si lo pide igual, error
  de protocolo y se corta la conexión.
- **Los protocolos nuevos nacen ocultos** para todos menos para el escritorio oficial de pruebas, hasta que se
  revisan (Umbriel).

### El bloqueo, partido en dos

S10 dijo que el bloqueo es del compositor, por lo que pasó en shiva: con el escritorio caído, el bloqueo se quedó sin
entrada. Aquí se mantiene y se refuerza:

- **El compositor bloquea**: tapa todas las salidas, corta la entrada a las ventanas y la recupera solo si la
  contraseña es buena. La comprueba el servicio de cuentas, no el escritorio.
- **El compositor dibuja el campo de la contraseña** y recibe las teclas. Ningún escritorio las ve.
- **El escritorio pone la cara**, si tiene el poder: fondo, reloj, avisos pendientes, música. Es `ext-session-lock`
  sin el teclado.
- **Si el escritorio cae con la pantalla bloqueada**, el compositor sigue con su cara mínima y el campo. Nada se
  queda en rojo ni sin entrada.

En Linux, con `ext-session-lock`, el cliente que bloquea recibe la contraseña y la comprueba con PAM. Aquí no hace
falta: el escritorio no necesita la contraseña para nada.

### El canal de órdenes y ARexx

Para guiones y para escritorios que prefieran no hablar Wayland para todo:

- **Un canal de órdenes del compositor**, como el IPC de niri: peticiones (ventanas, pantallas, monitores, cambiar
  de disposición, mover una ventana) y un flujo de eventos que da el estado entero al conectar y después los cambios.
- **Cambios atómicos**: cada lote de eventos lleva un número de estado y deja el estado coherente. Nunca una ventana
  en una pantalla que ya no existe.
- **Formato que solo añade**: los campos no se renombran ni se quitan; una versión nueva solo suma.
- **El mismo canal es el puerto ARexx del compositor** (S10 ya pedía el puerto del escritorio). ARexx necesita
  permiso para hablar con otro programa (S6), así que un guion sin ese permiso no mueve ventanas.
- **El canal pide los mismos poderes** que el protocolo de Wayland. No es una puerta trasera: niri avisa de que su
  socket deja lanzar clientes con todos los poderes, y aquí no puede pasar.

## 4. Cómo se escribe un escritorio

### Las opciones

**A. Solo en Dash, con una capa declarativa compilada.** Un kit de escritorios en Dash: barra, paneles y módulos
descritos como en `.dashui`, compilados a nativo.

- Gana: un solo camino, el lenguaje del proyecto, rápido y con poca memoria.
- Cuesta: depende del backend ARM de Dash (H7). Cierra la puerta a quien escribe en otro lenguaje.

**B. Un protocolo por canales que use cualquier lenguaje.** El contrato es el protocolo de la sección 3. Quien
quiera escribe su escritorio en Rust, C, Dash, Zig, o con un intérprete propio.

- Gana: abierto de verdad. Así nacieron DMS (Go y QML) y Noctalia (C++ y Luau) sobre niri.
- Cuesta: escribir un escritorio desde cero es mucho trabajo si no hay un kit.

**C. Un intérprete en el sistema** (QML, JavaScript, Lua), como Quickshell.

- Gana: recarga en caliente al guardar, y es fácil empezar.
- Cuesta: lo que S10 ya descartó y lo que acaba de dejar Noctalia: memoria, un motor que cambia por debajo, roturas.

### Recomendación: B como contrato y A como kit oficial

- **El protocolo es la interfaz.** Está documentado en `docs/referencia/escritorio.md` (cuando exista) y versionado.
  Ningún escritorio, tampoco el oficial, usa nada fuera de él.
- **El kit oficial es Dash**: un módulo `escritorio` sobre el motor de interfaz con el modelo de MUI y aspecto
  Material 3 (S11). Barra, paneles, listas de ventanas y módulos son objetos con atributos y avisos, como en MUI. Se
  describen de forma declarativa, en Dash o en un fichero tipo `.dashui`, y se compilan.
- **Recarga rápida sin intérprete.** Como el estado vive en los servicios, reiniciar un escritorio es barato: se
  recompila y se relanza en menos de un segundo (meta sin medir), y no se pierde nada. El aspecto (colores, orden
  de los módulos, tamaños) va en texto y se relee al guardar, sin recompilar, como la configuración de niri.
- **Antes de H7**, el escritorio mínimo y los de prueba van en Rust, con la caja `dashos`.
- **Un tercero puede traer su intérprete** dentro de su escritorio, en su propio proceso. Sus poderes siguen siendo
  los que el usuario le dio. El sistema no trae ninguno.

### Módulos de terceros

En DMS y en Noctalia los complementos corren dentro del escritorio, con todos sus poderes. En COSMIC cada applet es
un proceso con su ventana, y el panel tiene que hacer de compositor.

**Recomendación: un módulo es un proceso aparte que da datos y una descripción, no píxeles.**

- El módulo del tiempo, por ejemplo, tiene el poder de la red y nada más. Manda al escritorio "icono: nubes; texto:
  14 °C; al pulsar: abrir esta página".
- El escritorio lo dibuja con su tema. Todos los módulos encajan con el aspecto, y el módulo nunca ve la pantalla.
- Se describen con un vocabulario pequeño: icono, texto, número, gráfico, lista, botón, menú. Si un módulo necesita
  más, abre su propia ventana.
- Si un módulo se cae, el escritorio deja su hueco vacío y sigue.
- Así un módulo hecho para un escritorio sirve en otro que hable el mismo vocabulario.

Cuesta un proceso por módulo. Con pocos KB por proceso en un micronúcleo, no debería pesar; falta medirlo.

### Presupuesto de memoria

La meta de S0 es menos de 128 MB en reposo para todo el sistema. Noctalia gastaba 300 MB por monitor con Qt.
**Meta provisional: el escritorio oficial, por debajo de 32 MB en reposo con un monitor.** Sin medir; se mide en la
fase E9.

## 5. La política de ventanas

### Las opciones

**A. En el compositor**, con las disposiciones de S10 (flotante, mosaico desplazable, maestro y pila, rejilla,
monóculo, pestañas). El escritorio elige la disposición de cada pantalla y da órdenes.

- Gana: las animaciones, el arrastre de pantallas a lo Amiga y la respuesta al teclado van dentro del plazo del
  compositor (S2). Es lo que hace niri. Más sencillo.
- Cuesta: quien quiera otra forma de colocar ventanas tiene que pedirla al proyecto.

**B. Enchufable dentro del compositor**, con guiones o complementos (KWin, Hyprland).

- Cuesta: código ajeno en el proceso de confianza. Descartada.

**C. Fuera, en otro proceso**, como river 0.4.

- Gana: cualquiera escribe su gestor de ventanas; si cae, la sesión sigue.
- Cuesta: un protocolo más, delicado, y todo es muy nuevo (marzo de 2026).

### Recomendación: A ahora, preparado para C

- **El compositor trae las disposiciones de S10**, con la forma natural de mezclar mosaico y flotantes que pidió el
  usuario. Cada pantalla elige la suya.
- **Por dentro, el compositor separa "decidir dónde va cada ventana" de "dibujar"**, con cambios atómicos, como
  river. Así, si un día hace falta, ese trozo puede salir a otro proceso con un poder "gestionar ventanas", sin
  rehacer el compositor.
- **Si sale fuera, con plazo**: si el gestor no contesta a tiempo, el compositor aplica su disposición por defecto.

### Las ventanas de la caja Amiga

Encaja con S4, sección 3:

- **Cada ventana de Intuition es una ventana del compositor**, con su programa marcado como de la caja Amiga. El
  escritorio la ve en su lista y en el dock como cualquier otra.
- **Cada pantalla propia (`OpenScreen`) es una pantalla de la lista de S10.** Arrastrar pantallas hacia abajo lo hace
  el compositor, porque tiene que ir a 60 cuadros.
- **La pantalla de Workbench es el escritorio** (S4). Un programa 68k que abre una ventana de fondo (`BACKDROP`) en
  ella va detrás de las demás ventanas, encima del fondo del escritorio.
- **Los sustitutos de Workbench del Amiga** (DOpus Magellan, Scalos) corren en la caja como programas normales: sus
  ventanas son ventanas. Lo que hacen con `workbench.library` va al servicio Workbench (abajo).
- **Prueba de que funciona: Trasfondo de 68k** pintando en el fondo de la pantalla de Workbench dentro de DashOS.

### Las ventanas de la caja Linux

Encaja con S7:

- El intermediario de la caja (como Sommelier) reenvía las ventanas normales (`xdg-shell`) al compositor. Salen como
  ventanas de DashOS, con su sitio en el mosaico.
- **La caja Linux es una sola aplicación para los poderes** (S6). Sus programas no reciben capas, bloqueo, captura
  ni lista de ventanas: el intermediario no los reenvía. Una waybar o un Noctalia dentro de la caja no pueden hacer
  de escritorio de DashOS sin que el usuario lo dé a mano (pregunta 8).

### Workbench como servicio

Lección de `workbench.library`: los sustitutos tenían que parchear sus vectores. En DashOS:

- **El servicio Workbench** guarda los AppIcons, AppWindows y AppMenus que registran los programas (nativos y de la
  caja, por el puente de `workbench.library`), y los iconos que el usuario dejó en el escritorio.
- **El programa Workbench** es la cara: iconos de volúmenes y programas en el fondo, cajones como ventanas. Es un
  programa aparte, como `LoadWB`, y se puede sustituir entero por otro que hable con el mismo servicio.
- **Cualquier escritorio puede usarlo o no.** Uno minimalista, a lo niri, no enseña iconos en el fondo; el oficial,
  sí (S10).

## 6. Instalar, cambiar y sobrevivir a una caída

### Instalar y cambiar

- **Un escritorio es un paquete de `Paq`** (S8) del tipo "escritorio". Declara sus procesos y los poderes que
  pide. Se puede publicar en Hangar.
- **Al instalarlo, el compositor pregunta** por los poderes, en su propia ventana. Los poderes "de escritorio" (capas,
  ver ventanas, cara del bloqueo) se dan juntos con un solo sí; los de aplicación (captura, red) se preguntan aparte.
- **`Prefs/Escritorio` elige el escritorio**, con Usar, Guardar y Cancelar (S10). El cambio es en caliente: el
  compositor quita los poderes al viejo, lo para, lanza el nuevo y le da sus poderes. Las ventanas no se mueven.
- **Cada usuario elige el suyo.** El escritorio corre como el usuario, no como el sistema.

### Si un escritorio se cuelga

- **El gestor de servicios lo vigila** (S14) y lo relanza. Mientras tanto, las ventanas siguen en su sitio y se
  pueden usar; solo faltan la barra y el dock.
- **Si cae tres veces en un minuto**, el compositor lanza el **escritorio mínimo**: está en `SYS:`, firmado, escrito
  en Rust, sin módulos ni complementos. Trae una barra simple con reloj, una lista de ventanas, un lanzador y un botón
  para volver a elegir escritorio.
- **El compositor también vigila el bucle de cada escritorio**, con el mismo "¿sigues ahí?" que usa con las ventanas
  (S2). Un escritorio que no contesta cuenta como caído.
- **El atajo de rescate** es del compositor y no llega a nadie más. Abre un menú que dibuja el propio compositor:
  volver al escritorio mínimo, elegir otro, bloquear, cerrar la sesión. Sirve aunque el escritorio esté vivo pero
  roto (por ejemplo, una capa a pantalla completa que tapa todo). La combinación es la pregunta 9.
- **Una capa que tapa toda la pantalla** sin el poder "encima de todo" no se acepta. Con él, el atajo de rescate la
  quita siempre.

### Temas compartidos

- **El servicio de apariencia** lleva los "tokens" de Material 3: colores, tipografía, radios de las esquinas,
  separaciones, velocidad de las animaciones, iconos y punteros. Saca los colores del fondo de pantalla, como matugen
  en DMS. Se guarda en `ENV:` y `ENVARC:` (S10, S4b).
- **Todos leen lo mismo**: el compositor para los bordes, el escritorio para la barra y los paneles, el motor de
  interfaz (S11) para las aplicaciones. Cambiar de escritorio no cambia los colores de las aplicaciones.
- **Un tema es un paquete de datos, sin código** (S10). Se instala con `Paq` y sirve para cualquier escritorio.
- **Un escritorio puede traer extras propios** (formas de la barra, efectos), pero el oficial y los que quieran
  encajar respetan los tokens.
- **El fondo de pantalla lo elige el usuario**, así que el servicio de apariencia lo puede leer para sacar los
  colores sin leer la pantalla. El escritorio lo dibuja en su capa de fondo. Para un fondo de vídeo, el escritorio lo
  pasa por el decodificador (S9, fase B).

## 7. El escritorio oficial

Será uno más, con tres reglas:

- **Usa solo el protocolo público.** Si necesita algo que no está, se añade al protocolo, para todos.
- **Es un paquete** como los demás, que viene instalado y se puede quitar o cambiar.
- **No tiene más poderes** que los que tendría uno de terceros con los mismos sís del usuario.

Cuando llegue el vídeo del usuario, cada idea se coloca en una de tres cajas:

| Si la idea trata de... | Va en | Ejemplos |
|---|---|---|
| Cómo se mueven, colocan o animan las ventanas y las pantallas | Compositor | Gestos, arrastre de pantallas, vista general, disposición |
| Lo que se ve encima: barra, dock, paneles, avisos, fondo | Escritorio oficial | Aspecto de la barra, lanzador, centro de control |
| Datos que comparten todos | Un servicio | Historial de avisos, colores, música |

Si una idea del vídeo no cabe en el protocolo (por ejemplo, un efecto que mezcla ventanas y barra), se decide ahí:
se amplía el protocolo o se lleva al compositor. Lo que necesite el oficial para el vídeo está, por regla, a mano de
los demás.

### Lo que enseña el vídeo del usuario (2026-11-08)

El usuario pasó un vídeo de 89 segundos (1280x720, no va al repositorio) con el escritorio que quiere como oficial. Se
miró un fotograma cada 4 segundos; puede haber detalles entre medias que no se vieron. El fondo es un cuadro (Hopper,
"Nighthawks") y el tema, oscuro con acentos naranjas.

| Lo que se ve | Va en | Qué pide |
|---|---|---|
| Un marco con esquinas redondeadas alrededor de toda la pantalla; la barra forma parte del marco | Escritorio, con ayuda del compositor | Zona exclusiva en los cuatro bordes y esquinas redondeadas del área de ventanas |
| Cuatro estilos de barra que se cambian en vivo: "Island" (pastillas que se abren), "Classic", "Dashboard" (una pastilla que crece) y "Vertical" (barra y paneles pegados) | Escritorio | Nada nuevo: son capas suyas |
| Barra vertical a la izquierda: lanzador, espacios de trabajo como puntos, foto del usuario, reloj en vertical, grabación con su tiempo, avisos, brillo, red, Bluetooth, volumen, batería y apagado | Escritorio, con datos de los servicios | Servicios de red, sonido, energía, Bluetooth y avisos; estado de los espacios de trabajo |
| Isla arriba en el centro: hora, fecha, canción que suena con su onda, red, batería | Escritorio | Servicio de música (con la carátula) |
| Lanzador con buscador, categorías con su número (Todo, Favoritos, Recientes, Internet, Desarrollo, Juegos, Multimedia, Gráficos, Oficina) y favoritos con estrella | Escritorio | La lista de programas y sus categorías, del servicio de paquetes (S8) |
| Panel de ajustes rápidos pegado a la barra: modo noche, modo juego, no molestar, pantallas, deslizadores de brillo, volumen y micrófono, uso de CPU, memoria y temperatura | Escritorio | Poder de cambiar brillo, sonido y "no molestar" a través de los servicios; el modo noche (tono de color) lo aplica el compositor |
| Centro de control que cae desde arriba, con pestañas: reproductor con carátula, calendario, avisos ("Recording started"), portapapeles con imágenes, barras del sistema | Escritorio | Servicio de portapapeles con su propio poder (leer el portapapeles es sensible, S6) |
| Grabación de pantalla con contador, y un temporizador de 5 minutos | Compositor graba; escritorio solo lo pide y enseña el estado | La captura es del compositor, con su indicador que el escritorio no puede quitar (sección 2) |
| Selector de tema: claro u oscuro y paletas sacadas del fondo ("monochrome", "tinted"...) | Servicio de apariencia | Colores calculados del fondo, para todos los escritorios y programas |
| Selector de fondo a pantalla completa: la imagen grande, una tira de miniaturas con su paleta al lado y el resto desenfocado | Escritorio y servicio de apariencia | Desenfoque de lo que hay detrás de una capa: efecto del compositor |
| Una ventana que se abre con un zoom mientras el fondo se desenfoca | Compositor | Animaciones de ventanas y desenfoque, configurables |
| Menú de apagado: bloquear, cerrar sesión, suspender, reiniciar, apagar, con confirmación | Escritorio pide; sesión decide | Poder de pedir el apagado; la confirmación de lo que no se deshace la pone el sistema |

Lo que añade al compositor (fase E3 o E4): desenfoque detrás de las capas que lo pidan, esquinas redondeadas del
área de ventanas, animaciones de abrir y cerrar con zoom, y el tono de color del modo noche. Lo que añade a los
servicios: música con carátula, portapapeles con imágenes y paletas sacadas del fondo. Lo demás es del escritorio
oficial y no pide nada nuevo al protocolo. Se parece a lo que hacen hoy las shells de Quickshell sobre Niri; no se
comprobó de cuál sale el vídeo.

## 8. Fases y su prueba

Primero en QEMU `virt` con `ramfb` y teclado y ratón por xHCI, que ya funcionan (U6). Después en la Pi 4. Cada fase
tiene una prueba que se puede repetir sin manos: QEMU con su monitor (QMP) manda teclas y saca capturas.

| Fase | Qué | Prueba | Con |
|---|---|---|---|
| E1 | Compositor que habla Wayland sobre canales: `xdg-shell` y memoria compartida | Dos clientes de prueba en Rust, cada uno con su ventana; captura por QMP comparada con la esperada | H3 |
| E2 | Poderes por conexión y capas | Una barra de prueba reserva su franja y las ventanas se apartan. Un cliente sin el poder no ve el protocolo; si lo pide igual, se le corta | H3 |
| E3 | Lista de ventanas, órdenes y canal de órdenes con estado y cambios atómicos | Un guion abre tres ventanas, el dock de prueba las ve y activa la segunda; se comprueba el foco | H3-H5 |
| E4 | Bloqueo con campo propio | Bloquear, matar el escritorio, mandar la contraseña por QMP: se desbloquea. El registro demuestra que el escritorio no recibió ninguna tecla | H5 |
| E5 | Vigilancia, escritorio mínimo y atajo de rescate | Matar el escritorio tres veces: sale el mínimo y las ventanas siguen. Una capa que tapa todo se quita con el atajo | H5 |
| E6 | Servicios de avisos y apariencia; cambio de escritorio en caliente | Un aviso mandado antes del cambio aparece en el historial del segundo escritorio de prueba | H5-H7 |
| E7 | Kit de escritorios en Dash y módulos como procesos | Un módulo del tiempo sin poder de ficheros intenta abrir uno: se le niega y el escritorio sigue | H7 |
| E8 | Ventanas de la caja Amiga y pantallas arrastrables; ventanas de la caja Linux | Trasfondo de 68k en el fondo de Workbench; un programa 68k con pantalla propia que se arrastra; una ventana de la caja Linux en el mosaico | H5, S7 |
| E9 | El escritorio oficial, tras el vídeo; medidas en la Pi 4 | Memoria en reposo del escritorio (meta: menos de 32 MB) y 60 cuadros con carga (S0) | H7, H6 |

E1 a E3 no necesitan Dash ni la caja, y se pueden empezar en cuanto el compositor deje de ser el provisional de C50.

## Recomendación, resumida

1. **Tres capas**: un compositor de confianza (pantalla, entrada, foco, ventanas, bloqueo, atajos reservados y
   ventanas del sistema), servicios sin pantalla que guardan el estado (avisos, bandeja, música, apariencia,
   Workbench), y escritorios sin privilegios que lo enseñan.
2. **El protocolo es público**: Wayland estándar donde exista (`wlr-layer-shell`, `ext-session-lock`,
   `ext-foreign-toplevel-list`, `ext-workspace`) y una extensión propia para lo demás (órdenes de ventanas, menús del
   programa, atajos, espejos). Más un canal de órdenes con estado y cambios atómicos, que es también el puerto ARexx.
3. **Poderes por conexión**, por la marca del canal y un registro que solo cambia el usuario. Sin el poder, el
   protocolo no existe. Ningún escritorio lee el teclado ajeno, captura la pantalla ni inyecta entrada.
4. **El bloqueo es del compositor**, que dibuja el campo de la contraseña. El escritorio solo pone la cara.
5. **Cualquier lenguaje por el protocolo; el kit oficial en Dash**, compilado y declarativo, sin intérprete.
   Módulos de terceros como procesos que dan datos, no píxeles.
6. **La política de ventanas en el compositor**, partida por dentro para poder sacarla a otro proceso más adelante.
7. **Escritorios como paquetes**, cambiables en caliente; escritorio mínimo y atajo de rescate si algo falla; temas
   como datos que valen para todos.
8. **El escritorio oficial es uno más**, sin nada privado.

## Lo que queda abierto

Decidido por el usuario el 2026-11-08: todas como se recomienda ("vale a todo").

1. ¿Protocolo público para escritorios de terceros desde el principio? Recomendado: sí, marcado "inestable" hasta
   DashOS 1.0, para poder corregirlo.
2. ¿Wayland estándar más una extensión propia, o un protocolo todo propio? Recomendado: estándar donde exista, para
   que portar Noctalia o DMS sea posible algún día.
3. ¿Kit oficial en Dash compilado, sin intérprete en el sistema, y el resto de lenguajes por el protocolo?
   Recomendado: sí.
4. ¿Política de ventanas solo en el compositor, o enchufable fuera como river 0.4? Recomendado: en el compositor,
   partida por dentro para abrirla más tarde si alguien lo pide.
5. ¿El compositor dibuja el campo de la contraseña y el escritorio solo la cara del bloqueo? Recomendado: sí. Afina
   lo que decía S10.
6. ¿Módulos de terceros como procesos que dan datos y no píxeles, aunque limite lo que pueden dibujar? Recomendado:
   sí; el que necesite más, que abra su ventana.
7. ¿Workbench como servicio y programa aparte, que cualquier escritorio usa o no? Recomendado: sí, como `LoadWB`.
8. ¿Dejar que un escritorio de la caja Linux (waybar, Noctalia) haga de escritorio de DashOS? Recomendado: no de
   serie; quizá como opción avanzada, con los poderes dados a mano.
9. ¿Qué atajo de rescate? Recomendado: Ctrl + Alt + Supr, que en Windows ya es la "secuencia de atención segura" y no
   llega a ningún programa. Ctrl + Amiga + Amiga queda como reinicio, como en el Amiga.

## Cómo encaja

- Con S0: principio 1 (el compositor no se traba; el escritorio puede caer sin que se note en las ventanas) y
  principio 4 (un protocolo para todos los escritorios). La meta de memoria pide escritorios ligeros.
- Con S2: el compositor tiene la clase de plazo; el escritorio y sus módulos, la clase justa.
- Con S4: ventanas y pantallas de la caja Amiga como ventanas y pantallas del compositor; `workbench.library` de la
  caja habla con el servicio Workbench.
- Con S6: poderes por aplicación y panel para quitarlos; la marca del canal; ventanas del sistema dibujadas por el
  compositor.
- Con S7: la caja Linux solo reenvía ventanas normales.
- Con S8: un escritorio y un tema son paquetes; Hangar los puede vender.
- Con S9: el compositor habla Wayland sobre canales; los planos del HVS y la GPU son suyos.
- Con S10: lo amplía. Cambian el bloqueo (campo del compositor), Workbench (programa aparte), los módulos
  (procesos) y el estado (en servicios). Se mantienen las pantallas, la mezcla de mosaico y flotantes, Prefs y el
  aspecto Material 3.
- Con S11: el kit de escritorios va sobre el motor de interfaz con el modelo de MUI.
- Con S14: el gestor de servicios vigila el escritorio.
- Con S16: el compositor es un proceso, fuera del kernel.

## Fuentes

- niri: [README](https://github.com/YaLTeR/niri), [versiones](https://github.com/YaLTeR/niri/releases),
  [IPC](https://github.com/YaLTeR/niri/wiki/IPC), peticiones y eventos en
  [niri-ipc](https://docs.rs/niri-ipc/latest/niri_ipc/enum.Request.html) y
  [eventos](https://docs.rs/niri-ipc/latest/niri_ipc/enum.Event.html),
  [modelo de seguridad](https://niri-wm.github.io/niri/Security-Model.html),
  [configuración](https://niri-wm.github.io/niri/Configuration%3A-Introduction.html).
- Protocolos y quién los tiene: [security-context-v1](https://wayland.app/protocols/security-context-v1),
  [wlr-foreign-toplevel-management](https://wayland.app/protocols/wlr-foreign-toplevel-management-unstable-v1),
  [ext-workspace-v1](https://wayland.app/protocols/ext-workspace-v1),
  [ext-image-copy-capture-v1](https://wayland.app/protocols/ext-image-copy-capture-v1).
- Noctalia: [documentación](https://docs.noctalia.dev/),
  [anuncio de la v5](https://noctalia.dev/blog/announcing-noctalia-v5),
  [arquitectura](https://github.com/noctalia-dev/noctalia-shell/blob/main/ARCHITECTURE.md),
  [seguridad de Umbriel](https://docs.noctalia.dev/umbriel/security/).
- DMS: [visión general](https://duxlabs.dev/docs/dms/overview),
  [compositores](https://duxlabs.dev/docs/dms/compositors),
  [backend en Go](https://github.com/AvengeMedia/DankMaterialShell/tree/master/core).
- Quickshell: [web](https://quickshell.org/), [cambios](https://quickshell.org/changelog),
  [tipos](https://quickshell.org/docs/v0.3.2/types/),
  [repositorio](https://git.outfoxxed.me/outfoxxed/quickshell/src/branch/master).
- KDE: [scripts de KWin](https://develop.kde.org/docs/plasma/kwin/), [revisión D22571](https://phabricator.kde.org/D22571),
  [serviceutils.h](https://lxr.kde.org/source/plasma/kwin/src/utils/serviceutils.h).
- GNOME: [extensiones y roturas](https://gjs.guide/extensions/overview/updates-and-breakage.html),
  [el paso a GNOME 45](https://gjs.guide/extensions/upgrading/gnome-shell-45.html).
- COSMIC: [cosmic-panel](https://github.com/pop-os/cosmic-panel),
  [xdg-shell-wrapper](https://github.com/pop-os/xdg-shell-wrapper).
- Hyprland: [complementos](https://wiki.hypr.land/0.35.0/Plugins/Using-Plugins),
  [versiones que no casan](https://discourse.nixos.org/t/flakes-hyprland-and-hyprland-plugins-version-mismatch/38834),
  [foro](https://forum.hypr.land/t/hyprland-plugins-more-seamless-update-process/974.md).
- river: [repositorio](https://codeberg.org/river/river),
  [river-window-management](https://isaacfreund.com/blog/river-window-management/).
- Amiga: [cambios de la V44 de Workbench](https://codewiz.org/pub/articles/V44_Changes/V44_Changes_guide/node0007.html),
  [Scalos](https://se.aminet.net/util/wb/ScalosPlugin39.readme),
  [DOpus Magellan como sustituto](https://bigbookofamigahardware.com/amigareport/www.amigareport.com/ar505/news42.html),
  [DOpus 5.103](https://newreleases.io/project/github/BlitterStudio/dopus5/release/v5.103),
  [Ambient en MorphOS](https://www.osnews.com/story/15209/morphos-the-lightning-os/), [Ambient](https://zapek.com/software/ambient/).
- Trasfondo: su README (`~/Projects/trasfondo`): piezas sueltas (barra, panel, dock, ventanas) que se arrancan o no,
  puerto ARexx y colores del fondo. Es el gusto del usuario, y encaja con escritorios hechos de piezas.

Lo que no se comprobó: nada compilado ni medido. Que el escritorio de la caja Amiga (Workbench de 68k, DOpus) se
porte bien con el servicio Workbench es una suposición. Los procesos por módulo "pesan poco" está sin medir. Que
Intuition sea una biblioteca y Workbench una tarea aparte, y que Ctrl + Alt + Supr sea la secuencia de atención segura
de Windows, se dice de memoria.
