# Trasfondo nativo: el monitor del sistema integrado en el escritorio

Lo pidió el usuario: "haría falta una herramienta nativa como Trasfondo", y luego "integrada". Trasfondo es su monitor
de recursos para el Workbench del Amiga (`~/Projects/trasfondo`, en Dash). Aquí se escribe nuevo, en Rust: de
Trasfondo solo se miró qué enseña y cómo lo coloca. De DMS, solo la idea de los indicadores de la barra. No se leyó
código de Linux, procps, htop, GNOME, KDE ni DMS.

Rama `trasfondo` (worktree `.worktrees/trasfondo`), desde 486d053.

## Lo de debajo

- **Llamada 52, `cpu_tiempo`** (`kernel/src/llamadas.rs`, `cupo.rs`, `base/planificador.rs`). El planificador suma lo
  que corre cada hilo, en cada cambio, a su grupo y a todos los de encima (sumas atómicas, sin cerrojos ni memoria; las
  dos clases). Con la llave del grupo (leer) se leen los µs; con 0, los del grupo propio. No baja al matar el grupo. Solo
  quien tiene la llave mide a otro: `sesiones`, de lo que lanza.
- **Llamada 29**: el contador 3 (µs ociosos de un núcleo) cuenta también el rato ocioso de ahora. Antes, un núcleo
  dormido sin tic (M5) no sumaba nada hasta despertar y parecía ocupado al 100 %. El 14 da los núcleos en marcha.
- **`sesiones`, `MONITOR`** (24): la lista para el monitor, con la privacidad decidida por el usuario. Con detalle, lo
  del usuario de quien pide (el fondo, la barra y el centro miran por la persona de delante); las cajas Amiga y Linux,
  por su nombre (son del escritorio); el resto del sistema y las demás personas, en dos sumas sin marca ni nombres. La
  primera página, como mucho una vez cada 200 ms por programa (`OCUPADO`). Nadie lee la memoria ni los argumentos de
  otro: solo cifras del kernel (CPU y memoria cobrada a su grupo).
- **`MATAR` desde el centro**: el centro de control cierra por la persona de delante, solo lo suyo; sin nadie delante,
  nada. Lo demás del monitor no mata.
- **Red**: `R_ESTADO` lleva los bytes que entran y salen por la tarjeta (también los de la caja Linux). El servicio de
  red se lanza ahora antes del fondo (el primero que nombra un canal lo sirve), y el fondo tiene la llave de
  "red-estado" (solo leer). Cambian las llaves del panel rápido (3 red-estado, 4 centro, 5 sonido).
- `usuario/src/monitor.rs`: las cuentas sin dibujo (uso por núcleo y por proceso, red por segundo, historial de 60, los
  seis que más, ajustes de `Prefs/monitor`), probadas en el PC con contadores raros.

## Lo que se ve

1. **El panel en el fondo** (`usuario/src/fondo_panel.rs`, lo mueve `fondo`): una capa de fondo creada tras las franjas
   de la imagen (encima de ella y debajo de los iconos del Workbench, que van en la capa de abajo), sin región de
   entrada. Tarjeta de Material 3 en el contenedor bajo del tema, translúcida, con Roboto: la hora grande y la fecha,
   CPU por núcleo con barras y la gráfica de la máquina, memoria, los seis que más gastan (las cajas con su pastilla 68K
   o LINUX en los colores de `wayland::escena::marca`), el disco del sistema y la red (entra y sale por segundo, con su
   gráfica). En una pantalla baja (800 × 600) quita primero las gráficas y luego la hora, los discos y la red hasta que
   cabe.
   - El compositor conoce a cada proceso por la marca de su llave: una conexión por proceso. El panel va en la misma
     conexión que la imagen; el bucle de `fondo` espera en ella con el plazo de la próxima medida, y un hilo aparte
     recoge el canal "fondo" (el centro) y toca la puerta de la conexión. (Primero se probó un hilo con su propia
     conexión: el compositor lo tomaba por el mismo cliente y daba "número de objeto nuevo malo".)
2. **La barra**: una píldora a la izquierda de la campana con la CPU (un chip) y la memoria (una tarjeta), medidas cada
   2 s. Un clic abre un desplegable con los procesos (los diez que más gastan) y un botón "Abrir el monitor del sistema"
   que abre Ajustes en esa sección (la barra tiene ahora el canal "centro": llaves 4 centro y 5 avisos).
3. **Ajustes, "Monitor del sistema"**: la lista en columnas (proceso, CPU, memoria, usuario · grupo), cada 2 s, sin
   perder la fila elegida al reordenar. Intro en la cabecera cambia el orden (CPU, memoria, nombre, usuario). "Cerrar
   proceso" (o Supr) lo pide a `sesiones`. Las listas del kit admiten columnas con tabuladores.
4. **Ajustes, "Panel del escritorio"** (como TrasfondoPrefs): enseñarlo, la esquina, cada cuánto se mide (1, 2, 5,
   10 s), las gráficas y cada bloque. Se guarda en `Prefs/monitor` y el centro se lo dice al fondo ("monitor").

## Pruebas

- `make prueba`: `grupos` prueba la llamada 52 (un hijo que gira cuenta en su grupo y en el de arriba, no se pierde al
  matarlo, sin leer / otra llave / una que no existe no se lee) y la 29 (núcleos en marcha, uno que no existe, µs
  ociosos que no bajan). `monitor.rs` en el PC: núcleos que aparecen y desaparecen, contadores que dan la vuelta,
  procesos que nacen y mueren entre medidas, medidas muy seguidas, páginas mal formadas, ajustes raros.
- `hostil` (arranque-virt): ve con nombre solo lo suyo, las sumas sin nombres, no ve al compositor ni a `sesiones`,
  pedir otra vez en el acto da `OCUPADO`, cien seguidas no tumban nada, una página lejana viene vacía, no cierra una
  caja ni una suma.
- `arranque-virt`, con clics de verdad: la barra lanza `Programas/espera` (nuevo, un proceso de Ana que dura 90 s), un
  clic en la píldora abre los procesos, su botón abre el monitor; Supr sobre `espera` lo cierra y sobre la suma del
  sistema no; Intro en la cabecera ordena por memoria; en el Panel del escritorio se quitan y se ponen las gráficas y el
  fondo relee los ajustes.

- **La imagen para usar** (`make virt-uso`, sin ventana, 1920 × 1080, tableta USB): entrada como Ana con `sendkey`, el
  panel en el fondo, un clic en la píldora de la barra (el desplegable) y otro en "Abrir el monitor del sistema" (Ajustes
  en el Monitor, con sus columnas). Capturas en `capturas-agente/trasfondo-fhd-uso-*.png`.
- Capturas de `arranque-virt` en Full HD: `capturas-agente/trasfondo-fhd-monitor.*.png` (desplegable, monitor, ajustes
  del panel y el panel sin gráficas); en 800 × 600, `trasfondo-800-monitor.*.png`. Con Full HD falla, como siempre, la
  línea de la pantalla de 800 × 600.

## Medido

- **Lo que gasta el panel en reposo**, con la llamada 52 sobre el grupo del fondo (lo apunta el propio fondo cada
  minuto), con intervalo de 2 s, en QEMU (TCG, sin aceleración: en la Pi irá más holgado): entre 0,45 % y 0,77 % de un
  núcleo, en 800 × 600 (panel de 300 × 500) y en 1920 × 1080 (380 × 740). Antes del panel, el fondo en reposo no
  gastaba nada: esperaba en su canal sin plazo. Queda por debajo del 1 % pedido, pero no muy lejos: cada medida repinta
  el panel entero; si hiciera falta, se repintaría solo lo que cambia.
- `sesiones` atiende cada medida con unas 9 páginas de `MONITOR` (lo que gasta no entra en la cifra de arriba).

## Decisiones para el usuario

- **Privacidad** (como la pidió): con detalle solo lo de quien mira; el sistema y las demás personas, sumados y sin
  nombres. Las cajas salen por su nombre si son del sistema o de quien mira; la de otra persona, sumada.
- **Los programas del escritorio son de Ana**: `inicio` lanza el fondo, el centro, el Workbench, el explorador, el
  selector y la caja Amiga con la ficha de Ana (`FICHAS`). Por eso salen como procesos suyos y Ana los puede cerrar
  desde el monitor (también el propio centro). Es lo que dice la regla ("si tiene derecho"), pero quizá convenga
  marcarlos como "del escritorio" y no dejar cerrarlos desde ahí.
- **El uso de CPU de cada proceso es del total de la máquina** (los cuatro núcleos suman 100 %), no de un núcleo como en
  `top`. Así la suma de la lista cuadra con la cifra de la barra.
- **Ajustes crece a 720 × 480** (antes 560 × 420), con cinco filas en la lista, para que quepan las columnas.
- **El orden de lanzamiento cambia**: la red va antes del fondo (para que el fondo tenga "red-estado"), y cambian las
  llaves del panel rápido y de la barra.

## Pendiente

- Temperatura: no hay quien la dé (en la Pi saldría del buzón de la VideoCore; hace falta un canal de solo lectura).
  El panel no la enseña.
- Las tareas de exec dentro de la caja Amiga y los procesos de la caja Linux: no se ven (la caja sale como un proceso).
  Haría falta un canal de solo lectura de la caja con su lista de tareas.
- Discos: solo el disco del sistema (INFO de `archivos`); los volúmenes de dentro comparten su espacio. Los ADF y los
  discos de la caja Linux no salen.
- Los nombres de usuario del monitor son provisionales en el centro, como las cuentas de `sesiones`.
- `PROCESOS` y `MEMORIA` de `sesiones` (los de antes) siguen dando con nombre los procesos del sistema: no se tocaron
  para no romper lo que los usa. La decisión de privacidad del usuario pide revisarlos.
