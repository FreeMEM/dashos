# Bitácora: el Workbench nativo, primera versión (S10)

Rama `workbench`, sacada de `revision`. Diseño en `docs/diseño/S10-escritorio.md` ("Workbench", "Aspecto") y
`docs/diseño/S6b-usuarios-y-permisos.md` (raíces, permisos, `Home:`).

## 2026-11-12: estudio (primera vuelta, cortada por el cupo)

Se estudió lo que había y se decidió cómo hacerlo, sin código. Lo decidido está abajo.

## 2026-11-12: primera versión

Antes, `git merge revision` dos veces (la segunda, por los arreglos del guion de `revision`).

### Decisiones provisionales (las tomó la sesión principal; las confirma el usuario)

1. **Capa de abajo (1), no de fondo (0).** Una columna de 104 de ancho pegada a la derecha, bajo la barra (anclas
   arriba, abajo y derecha, sin zona). Búfer ARGB transparente salvo los iconos: un fondo de pantalla (rama
   `fondo-c3`) en la capa 0 queda debajo y se ve. Sin teclado: un clic en un icono no quita el teclado a nadie.
   Primero fue una capa de toda la pantalla, pero el compositor mezcla lo transparente en cada composición, y con el
   PC cargado la prueba de virt no llegaba al final. Para iconos sueltos por todo el escritorio y AppIcons hará falta
   la pantalla entera, y que el compositor se salte lo transparente.
2. **Iconos arriba a la derecha.** El guion mueve el ratón en diagonal desde arriba a la izquierda y se queda clavado
   abajo a la derecha. Ahí no pasa.
3. **`Home` provisional.** La ficha es como la del selector: Ana (1000), con el derecho de repartir y la raíz del
   disco. Al empezar se cede a sí mismo el disco solo para leer (raíz 1, "Sistema") y `C` para leer y escribir (raíz
   2, "Home"), y solo va por ellas. El `Home:` de verdad es el volumen cifrado "Ana": abrirlo por su nombre pide el
   derecho de administrar, y en el alta no sirve porque al arrancar está cerrado. Tendrá que cedérselo `sesiones` al
   entrar Ana. El icono de `Home` sale solo mientras la sesión de Ana está abierta (`ACTIVA`, cada 1,5 s).
4. **Dueño "usuario 1000 (tú)".** `sesiones` no da los nombres de las cuentas.

### El programa: `usuario/src/bin/workbench.rs`

- Lo lanza `inicio` en virt, al final de la lista de `kernel/src/inicio.rs` (así no cambian los números de los de
  antes), con "ventanas", "archivos" y "sesiones". Su ficha, en `FICHAS` de `usuario/src/bin/inicio.rs`, y 48 MB de
  cupo (cada ventana lleva su búfer y las fuentes del kit). El compositor le da las capas por su nombre (`CON_CAPAS`).
- **Escritorio**: Sistema y, con la sesión de Ana abierta, Home. Doble clic abre el volumen; botón derecho, su
  Información. Lo elegido va sobre el contenedor secundario; bajo el ratón, una capa de la superficie. El nombre, sobre
  una pastilla, para que se lea sobre cualquier fondo.
- **Ventanas de cajón** (xdg_toplevel de 440 × 420, con el kit): el camino como título y "Cerrar"; debajo, cuántos hay
  (o un aviso), "Lista"/"Iconos" e "Información"; y el contenido, que pinta el programa en un hueco del kit (clase
  `Hueco`). Rejilla de 96 × 92, o lista con cabecera (Nombre, Tamaño, Fecha) y filas de 32. Un clic elige, doble clic
  abre, botón derecho da la Información. Teclado: flechas, Inicio, Fin, Intro y Esc (cierra). Se desplaza con lo
  elegido; no hay rueda (el compositor no manda `axis`).
- Doble clic en un cajón abre otra ventana. Se busca de un paso (la asa del cajón y un nombre) y se mira que siga
  siendo un cajón. Sin permiso, un aviso en la ventana ("Sin permiso para abrir DeCarla").
- Doble clic en un programa de `Sistema/Programas`: lo lanza `sesiones` como la persona de delante (orden `LANZAR`,
  como la barra). Otros ficheros no se abren todavía: no hay herramienta por defecto.
- **Información** (460 de ancho, el alto que pide el kit): nombre, tipo y tamaño, fecha, dueño y grupo; el comentario
  en un campo con etiqueta; y una rejilla de casillas propia (clase `Permisos`): filas Dueño, Grupo y Otros con Leer,
  Escribir, Ejecutar y No borrar (bits 16, 32 y 64 del Amiga), y la fila Amiga con Guion, Puro, Archivado y Residente.
  Guardar manda `COMENTAR` y `PROTEGER` si cambiaron, y pone al día las ventanas de ese cajón. Si no es de Ana, o se
  llegó por Sistema (solo lectura), todo desactivado, un aviso y solo "Cerrar".
- **Iconos propios** (volumen, cajón, fichero, programa), con formas redondeadas y los tonos del tema: el disco en el
  contenedor primario (Home en el terciario, con un tejado), el cajón con pestaña y fondo en el primario y el frente
  en su contenedor, la hoja con la esquina doblada y renglones, y el programa como un cuadrado redondo con un
  triángulo de "en marcha". Siguen el tema del sistema.
- **Nombres raros**: vacíos, ".", "..", con '/', ':', '|' o de control, ni se enseñan ni se buscan.
- Si se vuelve a una instantánea del disco (la prueba FS9 de `ficheros` lo hace), el servicio suelta las raíces de
  todos: el Workbench se las vuelve a ceder la primera vez que una le da "asa mala".
- Fechas en UTC ("12/11/2026 10:04"): aún no hay zona horaria.

### Lo que cambia fuera del programa

- `archivos`: orden nueva **34 `DUENO`**: dueño (a) y grupo (b) de una asa, y el usuario de quien pregunta (c). Buscar
  y `DATOS` no daban el dueño. En `docs/referencia/servicios.md`.
- `wayland/src/escena.rs`: la cascada ya no saca ventanas de la pantalla. Una ventana que se saldría por la derecha o
  por abajo se mete dentro de lo que dejan las barras (la Información salía con sus botones fuera). Prueba de escena:
  `la_cascada_no_saca_ventanas_de_la_pantalla`.
- Kit: `Interfaz::con_contexto`, para reusar el contexto (las fuentes ya leídas) de una ventana cerrada. Cada
  `Interfaz::new` lee Roboto dos veces; con cinco ventanas, el Workbench se quedó sin memoria ("memory allocation of
  8192 bytes failed"). Prueba: `con_un_contexto_de_antes`.
- `sesiones`: "workbench" en `LANZADORES` (solo eso).
- Disco de pruebas (`tools/disco_archivos.py`): `DeCarla/` (de 1001, 0700) con `carta.txt`, en la raíz, e
  `Imagenes/raro<tabulador>dos.txt` (RedoxFS no admite ':'). En la raíz y en Imagenes, no en `C`: así no cambia lo que
  ven el selector y el hostil. La raíz pasa de 6 a 7 entradas (la línea de `ficheros`). `--comprobar` espera además el
  comentario y el bit de archivado de `C/Privado/nota.txt` que pone la prueba. El fichero de Carla se llamó primero
  `secreto.txt`, y chocaba con la comprobación de FS10 (que ese nombre no esté en claro en el disco).
- Guion (`tools/prueba_uefi.sh`): si el selector ya se rindió (un minuto sin elegir), no se le espera más. Antes sus
  pasos hacían `continue` para siempre y el guion no llegaba ni a Super+L. Pasó con el PC cargado: la sesión siguió
  bloqueada todo ese minuto. Sus líneas faltan igual y lo dicen.
- `kernel/Makefile`: `arranque-virt`, de 200 a 300 s: el Workbench va al final y con el PC cargado no daba tiempo. Se
  para en cuanto salen todas las líneas.

### Pruebas

- `cd wayland && cargo test --lib`: 47 bien. `cd kit && cargo test --lib`: 46 bien.
- Virt, al final del guion (tras la barra), con el ratón: doble clic en Sistema, en DeCarla (no se abre: -12), en
  Programas y en hola (lo lanza `sesiones`); botón derecho en hola (Información, que no se puede cambiar) y Cerrar;
  doble clic en Home y en Privado, Lista, un clic en nota.txt e Información; en el campo "nota de Ana", la casilla
  Archivado y Guardar; y Cerrar en las cuatro ventanas. Los sitios: la n-ésima "ventana de workbench" del compositor
  y lo que dice el Workbench de cada una.
- Hostiles, al arrancar: no lista el cajón de `sesiones` ni `DeCarla` (-12), no abre sus ficheros (-12), no crea por
  Sistema (-12) y ".." desde Home no se abre. Los nombres con '|' (Home) y tabulador (Imagenes) no salen; "..",
  "a/b" y "x:y" no se buscan.
- Capturas con `CAPTURA_WORKBENCH=fichero.ppm make arranque-virt`: `fichero.ppm` (Sistema y Programas),
  `.lista.ppm`, `.info.ppm` y `.comentario.ppm`. En `capturas-agente/` (fuera de git).

### Resultado

- `make prueba`: BIEN.
- `make arranque-virt`, once vueltas con el PC a una carga de 50 a 80. La última: todas las líneas del Workbench bien,
  y `disco_archivos.py --comprobar`, pasado a mano, BIEN (con el comentario y el bit de nota.txt). Falta una línea
  que no es del Workbench: "[ajustes] el desplegable dice: Destello" (en tres vueltas seguidas). Por qué: el selector
  sale justo cuando `pide` bloquea la sesión de Carla; el guion de `revision` lo cuenta como bloqueo y no le teclea, y
  su ventana se queda un minuto en (136, 172), encima del menú de Ajustes, que el guion pulsa en ese rato. Como no
  salen todas las líneas, no se hace la captura final y `captura_ventanas.py` no se comprobó.
- Fallos por el camino, ya arreglados: el Workbench no se conectaba al compositor la primera vez (se reintenta); la
  capa de pantalla entera retrasaba la prueba; ":" en un nombre rompía `dashfs-ar`; el guion se quedaba esperando al
  selector para siempre; sin memoria con cinco ventanas; las raíces dejaban de valer tras FS9; `secreto.txt` chocaba
  con FS10.

### Pendiente

- `Home:` de verdad: que `sesiones` ceda el volumen de la persona al entrar (hoy, `C`).
- El escritorio entero como capa (iconos en cualquier sitio, AppIcons, soltar ficheros), con un compositor que se
  salte lo transparente.
- Abrir ficheros con su herramienta (tooltypes, iconos `.info`), copiar, mover, borrar y renombrar, arrastrar.
- Rueda del ratón (el compositor no manda `axis`), menús del Workbench en la barra (botón derecho), ventanas que
  cambian de tamaño, recordar dónde estaban.
- Los nombres de las cuentas para el dueño.
- La carrera del guion entre el selector, el bloqueo de Carla y el menú de Ajustes (de `revision`).
