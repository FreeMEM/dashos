# Fondos: la herramienta, las transiciones y los fondos animados sin GPU

Rama `fondos`, de `revision` (ba0c2a2e). Lo pidió el usuario: "Nos hace falta una herramienta espectacular para cambiar
de fondo, poner fondos animados con un bajo consumo de CPU, que si lo calcula la GPU sería lo suyo". La GPU la diseña
la rama `gpu-diseno`; aquí, la herramienta, las transiciones y los fondos animados que van bien sin GPU, con el enganche
para la GPU preparado. De DMS (S27, MIT) solo se miró qué hace (el selector de fondos, el pase, claro/oscuro, por
monitor, el acento del fondo); no se copió código.

## Diseño en tres frases

`fondo` pinta cada pantalla con tres juegos de memoria por franjas (lo que se ve, la imagen quieta de debajo y, en una
transición, la nueva), y todo lo que se mueve (transición, animado, efecto) toca solo lo que cambia y pinta un cuadro
por aviso de `frame`. El compositor no da ese aviso a una capa de fondo que no se ve (tapada por ventanas opacas o
maximizadas, pantalla apagada o bloqueada), así que un fondo animado se para solo, y un ritmo con tope de CPU (que
frena con la máquina ocupada) pone cada cuánto. La herramienta va dentro de `fondo` (una capa encima de todo, abajo y a
lo ancho, con el escritorio de verdad arriba para la vista previa) y todo se guarda en `Escritorio/fondo` de
`ENV:`/`ENVARC:`, el mismo fichero que escribe la orden `Fondo` y que se puede editar a mano.

## Lo hecho

### `kit::fondos` (lo que no depende de DashOS, con sus pruebas de PC)

- `Marco`: la pantalla en trozos de filas (las franjas de 4 MB), sin tenerla seguida en memoria.
- `Ajuste`: llenar (el de siempre, igual píxel a píxel), encajar, centrar, mosaico y estirar (el que usa el usuario en
  su DMS). Bilineal con enteros; una imagen que encoge a menos de la mitad se reduce antes una vez.
- `transicion`: ninguna, fundido, barrido (cuatro lados, con borde suave de 64 px), círculo que crece desde un punto
  (borde suave de 28 px) y disolver (cuadros de 16 px en orden revuelto, a lo Amiga); "azar" elige una. Sin copia del
  fondo de antes: cada paso lleva cada píxel de la parte `a` de lo nuevo a la `b` (`(b-a)/(1-a)` de lo que le falta),
  que da lo mismo que mezclar el viejo y el nuevo. El ritmo lo pone el reloj (un cuadro tardío da un salto mayor, no
  alarga la transición), con curva suave. El barrido y el círculo solo tocan su franja o su anillo (dos rectángulos
  por banda de 16 filas).
- `animado`: el `trait Animado` (base propia o efecto encima de una imagen, `paso` con los daños, `olvidar`, y
  `shader()`: la versión para la GPU). Cuatro: **aurora** (cortinas verdes y violetas; repinta por turnos una de 4
  tiras de la franja del cielo), **luciérnagas** y **nieve** (partículas: cada cuadro restaura su rectángulo de antes
  de la base y pinta el de ahora) y **respira** (un degradado de tres colores que respira en 16 s; cada fila es un
  color: llenar; solo repinta si algún color cambia). Sus bases (cielo con estrellas y colinas, bosque de noche,
  montañas nevadas) se hacen aquí.
- `ciclos`: la paleta animada de Deluxe Paint sobre un PNG de paleta (`png::leer_indexada`; los tramos en un trozo
  `tEXt` `DashOS-ciclos`: `64-79 6`, `80-95 10 atras`). Solo se repintan los píxeles de los colores que giran, y los
  daños son los tramos de columnas de cada banda de 32 filas (juntando lo cercano): unas estrellas sueltas o el
  reflejo del sol no hacen componer toda la anchura.
- `ritmo`: el intervalo es el que pide la animación o más si así se pasaría del tope (3 % de un núcleo de serie),
  contando lo del fondo (por el reloj) y lo del compositor (40 ns por píxel dañado, lo medido en QEMU); con menos del
  25 % de la máquina ociosa, cuatro veces más despacio (vuelve por encima del 40 %).
- `ajustes`: el fichero `Escritorio/fondo` (fondo, oscuro, por pantalla, ajuste, color, transición, duración, pase,
  carpeta, efecto, cpu). Lo que no vale se queda en lo de serie y se avisa; escribirlo no pierde lo que no es suyo.
- `juntar_danos`: las partículas dan decenas de rectángulos; se juntan los cercanos hasta 32 sin cubrir de más (antes,
  uno que lo cubría todo: el compositor componía 1 Mpx por cuadro de luciérnagas).

### El programa `fondo`

- Una salida por `wl_output` (hasta 4), con su nombre (DASHOS-1...): fondo por pantalla y por modo (claro/oscuro: al
  cambiar el modo del tema, si hay otro fondo para el modo nuevo, se pone).
- Las transiciones y los animados van por avisos de `frame` (un cuadro tras otro, sin adelantarse al compositor); una
  transición cuyo aviso no llega en 1 s (no se ve) se acaba de golpe.
- `Escritorio/fondo` de `ENV:` vigilado (avisos de cambio) y aplicado en marcha; sin él, `Prefs/fondo` (lo de C3), y
  al entrar alguien se escribe en `ENV:` con eso (para que la herramienta y la orden partan de lo que se ve).
- El pase (cada N minutos, el siguiente de la carpeta), la paleta animada al leer un PNG de paleta con sus tramos, el
  efecto encima de una imagen, el fondo desenfocado de la pantalla de bloqueo (ahora de la base de la primera pantalla).
- Cada 10 s, con algo en marcha, dice lo que gasta (cuadros, píxeles por cuadro, lo de pintar y el programa entero).
- Los mensajes de C3 se quedan como estaban (las pruebas los miran): `ondas.png en pantalla: 960 × 600 escalada a ...`.

### La herramienta (`fondo_herramienta.rs`, dentro de `fondo`)

Capa de layer-shell encima de todo, pegada abajo y a lo ancho, con la mitad del alto y el teclado: arriba se ve el
escritorio de verdad, donde sale la vista previa. Fila de arriba: el título, las pantallas ("Todas" o una), Claro y
Oscuro, buscar (se teclea sin más) y cerrar. Debajo: las carpetas (Home:Fondos, SYS:Fondos y Animados), el ajuste, la
transición, el pase y el efecto. La rejilla de miniaturas (con la proporción de la pantalla) se hace en otro hilo, solo
lo que se ve y como mucho 96 guardadas; un fichero que no se lee sale con su porqué ("demasiado grande", "un trozo está
roto"). A la derecha, la elegida en grande, lo que mide y el color de acento que saldría (la semilla y los colores del
tema en el modo elegido, calculados en el hilo: 180 a 340 ms en QEMU). Ratón quieto 450 ms en una miniatura, o elegirla
con las flechas: vista previa en el escritorio con la transición puesta (el círculo sale de la miniatura: la capa sabe
dónde está). Aplicar (Intro, doble clic) guarda en `ENV:` y `ENVARC:`; Cancelar o Esc vuelven a lo de antes. Sin tocar
Claro ni Oscuro se aplica a los dos modos; tocando uno, solo a ese.

Se abre: Ajustes > Apariencia (fila "Fondos de escritorio: abrir la herramienta…", la última), el centro de control
(mosaico "Fondos", que se añade en su modo de edición), la orden `Fondo` del Shell y, por el canal del centro, con
"abrir fondos" (para la barra y el escritorio cuando lo usen).

### La orden `Fondo` (Shell)

`Fondo` (abre la herramienta), `Fondo LISTA`, `Fondo <imagen|animado:x> [PANTALLA DASHOS-2] [OSCURO|CLARO] [GUARDAR]`,
`Fondo SIGUIENTE`, `Fondo AJUSTE|TRANSICION|PASE|EFECTO|CPU|DURACION <valor>`. Escribe `ENV:Escritorio/fondo` (y
`ENVARC:` con `GUARDAR`); para abrir, `ENV:Escritorio/fondo-pide`.

### El compositor y el resto

- `escena::fondo_sin_ver`: el aviso de `frame` de una capa de fondo se retiene si la pantalla está apagada (Blanker) o
  bloqueada, o si lo de encima la tapa (búferes sin alfa y ventanas maximizadas; lo que queda a la vista, si son menos
  de 1024 píxeles y poco de la capa, no cuenta). Prueba de PC `fondo_tapado_sin_avisos`.
- El compositor dice también su CPU cada 10 s (llamada 52 sobre su grupo).
- El centro: al cambiar `ENV:Escritorio/fondo`, con la paleta del fondo, pide "guardado" a `fondo` y el tema sale del
  nuevo; "abrir fondos" por su canal.
- `inicio`: `fondo` lee el disco (`SYS:Fondos`, solo para leer) y su cupo pasa de 48 a 96 MB (tres juegos de 8 MB por
  pantalla, la herramienta y un PNG a medio leer). `sesiones`: `fondo` lee `ENV:` (como el centro y el dock) y puede
  escribir y guardar `Escritorio/fondo`.
- `SYS:Fondos/mar.png`: un atardecer sobre el mar de 640 × 400 con 256 colores y su paleta animada (el agua junto al
  horizonte, el reflejo del sol y las estrellas), hecho aquí por `tools/fondos_prueba.py` (8 KB, sin imágenes de nadie).

## Medido (QEMU `virt`, TCG sin aceleración, la máquina compartida con otras sesiones)

`fondo` dice cada 10 s, con algo en marcha, lo suyo ("pintar": lo que tardan los pasos, por el reloj; "el programa
entero": llamada 52 sobre su grupo, con su panel del monitor y sus hilos); el compositor, su CPU (llamada 52) y lo que
compuso. En milésimas de un núcleo, en reposo (nada encima), tras la transición:

| Fondo | 800 × 600: `fondo` | 800 × 600: compositor | 1920 × 1080: `fondo` | 1920 × 1080: compositor |
|---|---|---|---|---|
| quieto (atardecer.png) | 4 a 6 (su panel) | 7 | ~10 | 15 a 19 |
| aurora | 15 a 17 | 36 | 22 | 48 |
| luciérnagas | 11 | 21 a 23 | 21 a 38 | 132 a 156 (*) |
| nieve | 9 | 24 | 12 | 242 (*) |
| nieve como efecto sobre atardecer.png | 10 | 26 | — | — |
| respira | 5 | 61 a 73 | 10 | 103 |
| paleta animada (mar.png) | 8 a 10 | 71 | 14 | 85 |
| bloqueada (aurora sin verse) | 4, 0 cuadros | sin componer el fondo | 8, 0 cuadros | — |

- 800 × 600: la vuelta final (tope de serie 5 %, contando el compositor con 90 ns por píxel y 5 ms por cuadro).
- 1920 × 1080: la vuelta de antes del tope final (3 % y 40 ns por píxel), ya con el arreglo de `colocar_capa`. (*) Con
  ese tope las partículas iban a 15 cuadros por segundo y el compositor gastaba 10 ms por cuadro aunque fuera pequeño:
  por eso el tope cuenta ahora un coste fijo por cuadro, y la nieve tiene 57 copos en vez de 140. La vuelta final a
  1920 × 1080 salió con la máquina muy cargada por otras sesiones (el compositor componiendo 5 a 6 s de cada 10 hasta con
  el fondo quieto): sus números no valen para comparar (están en `consola-1920x1080.txt`); sus capturas, sí.
- En 800 × 600 todo lo que se mueve en reposo cuesta del 3 al 8 % de un núcleo entre los dos (`fondo` y compositor); lo
  que más, respira y la paleta animada (cambian mucha superficie). El objetivo de "pocos por ciento" se cumple en
  800 × 600; en 1920 × 1080 las partículas y la aurora quedan en el 5 al 7 % y respira y la paleta, en el 10 %.
- Lo que manda es el compositor (componer por CPU en QEMU: de 49 a 99 ns por píxel y unos 5 a 10 ms por cuadro aunque
  sea pequeño). El fondo pinta poco: la aurora, una de cuatro tiras del cielo; las partículas, 4 a 22 kpx por cuadro;
  la paleta, 105 kpx (800 × 600) o 300 kpx (1920 × 1080).
- Dos arreglos que lo bajaron mucho: (1) el compositor repintaba entera una capa en cada commit (`colocar_capa`): con
  la aurora pasó de 213-292‰ a 48‰ en el compositor; (2) los daños de las partículas se juntaban en uno que lo cubría
  todo (1 Mpx por cuadro): `juntar_danos`.
- Las transiciones a 1920 × 1080 (2 s para verlas): 8 a 10 pasos (de 4 a 5 por segundo); el fundido es el más caro
  (toda la pantalla en cada paso). En 800 × 600 el compositor contestaba tan deprisa que daba 270 pasos por segundo:
  ahora como mucho 40. En las capturas se ven a medias (`t-*-05` a `t-*-07`).
- Leer y poner un PNG: atardecer (640 × 480) 85 a 140 ms; mar.png 60 a 110 ms; un animado, 3 a 20 ms.
- Sin verse (pantalla bloqueada): "aurora: 0 cuadros en 10 s" y el compositor no compone el fondo; al desbloquear sigue.

## Pruebas

- PC (`cargo test` del kit, en `make prueba`): `kit::fondos` con hostiles: marcos mal troceados, imágenes raras (1 × 1,
  1 × 900, 3000 × 2000, 0 × 0) en cada ajuste y medida, cada transición acaba exactamente en lo nuevo y todo lo que
  cambia cae en sus daños (también con el centro fuera de la pantalla), transiciones a ráfagas (40 seguidas, cada una
  sobre la anterior a medias), medidas que no casan, cada animado con tiempos raros (saltos de una hora, hacia atrás,
  repetidos) y pantallas de 1 × 1 y 3 × 500 sin salirse de sus daños, partículas a 1920 × 1080 que tocan poco, tramos
  de paleta mal escritos (13 casos), paletas cortas e índices fuera, 40 tramos (se quedan 16), la paleta vuelve a como
  estaba tras una vuelta, PNG de paleta con su `tEXt` (y cien trozos de texto), el ritmo (tope, ocupada, costes
  absurdos), `juntar_danos` (mil iguales, vacíos, tope 1) y el fichero de ajustes (malos uno a uno, 2000 de basura al
  azar, escribir sin perder lo ajeno).
- Wayland (`cargo test`): `fondo_tapado_sin_avisos`: con una ventana opaca maximizada encima, la capa de fondo no recibe
  su aviso; al irse, sí; con la pantalla apagada, tampoco; y un commit con el mismo búfer compone solo su daño (falla
  sin el arreglo de `colocar_capa`).
- `make prueba`: BIEN. `cargo test` del Shell (la plantilla de `Fondo`): bien.
- `make arranque-virt`: BIEN (la raíz del disco lleva ahora `Fondos/`: 23 entradas). `make arranque-virt-2`,
  `-escritorio`, `-dms` y `-shell`: BIEN.
- `make arranque-virt-fondos` (nuevo, `tools/guion_fondos.py`, `kernel/pruebas/arranque_virt_fondos.txt`): BIEN a
  800 × 600. Entra Ana, un guion del Shell cambia de fondo con cada transición, `Fondo` abre la herramienta, Home:Fondos
  con sus miniaturas (las rotas, con su porqué), la vista previa al dejar el ratón quieto, los animados, la aurora en
  vista previa con su acento, Aplicar (`ENV:` y `ENVARC:` por `sesiones`), cada animado 25 s, y bloqueada: 0 cuadros.
  A 1920 × 1080 con el perfil de shiva (`DASHOS_PANTALLA=1920x1080 FONDOS_SHIVA=1 DASHOS_FONDO_EXTRA=...`): las
  capturas.

## Capturas (fuera de git)

`~/recuperar/geosian-7d/fondos-capturas/`: `1920x1080/` y `800x600/` (`t-<transición>-NN`: ráfagas de cada
transición; `p-paleta-*`; `h0` a `h5`: la herramienta, la vista previa, los animados, el acento y la aurora aplicada;
`m-*`: cada animado en reposo; `z-*`: bloqueada y desbloqueada) y las consolas. `fuente/fuji.png` es un fondo del
usuario (Fuji-Dark, reducido a 1920 × 1013) solo para las capturas: no va en git. Las de `v3/` y `v4/` son de vueltas
de antes (con fallos ya arreglados). No hay vídeo de verdad: las ráfagas de capturas de QEMU salen a 3 o 4 por segundo;
con ellas, `transiciones-1920x1080.gif` (las cuatro transiciones y la paleta animada).

## Decisiones que debe mirar el usuario

1. **La herramienta va dentro de `fondo`**, no en un programa aparte: la vista previa no necesita protocolo, y el
   permiso de tocar el fondo no sale de `fondo`. Si se cae, se cae con ella el fondo.
2. **Es una capa abajo, a lo ancho y con la mitad del alto** (el 78 % en pantallas de menos de 900 de alto), no una
   ventana: así se ve el escritorio de verdad arriba para la vista previa, y se sabe dónde está cada miniatura (el
   círculo sale de ella). Tiene el teclado mientras está abierta (como un diálogo).
3. **La orden `Fondo` pide abrir la herramienta escribiendo `ENV:Escritorio/fondo-pide`** (el Shell no tiene el canal
   de `fondo`): sirve, pero es un fichero de `ENV:` usado como aviso.
4. **Sin tocar Claro ni Oscuro, Aplicar pone el fondo de los dos modos** (y `Fondo x` sin `CLARO`/`OSCURO`, igual);
   tocando uno, solo el de ese modo.
5. **El tope de serie es 5 % de un núcleo contando el compositor** (por estimación: 90 ns por píxel y 5 ms por cuadro,
   lo de QEMU). En una Pi el compositor es mucho más rápido y el tope se quedará corto (los animados irán más lentos
   de lo que podrían): habría que medirlo allí o que el compositor diga lo que gasta.
6. **La fila de Ajustes > Apariencia va la última** (para no mover las de antes ni las pruebas que las cuentan).
7. **El mosaico "Fondos" del centro de control no está en los de serie**: se añade en su modo de edición.
8. **`SYS:Fondos/mar.png`** (8 KB) es el único fondo de ejemplo en el disco; los animados no ocupan disco.

## Lo que queda

- **El menú del escritorio**: el Workbench es de la rama `dock-discos` (lo está cambiando a la vez); el enganche está
  hecho (el centro atiende "abrir fondos"), falta que el Workbench tenga la llave "centro" y un menú con el botón
  derecho en el escritorio.
- **Arrastrar una imagen** a la herramienta: el compositor no tiene arrastrar y soltar entre programas (`START_DRAG` se
  cancela). Hace falta en el compositor (wl_data_device) y en el explorador.
- **JPEG**: los fondos del usuario son casi todos JPEG; solo se lee PNG. Para las capturas se pasó uno a PNG fuera de
  git.
- **Vídeo y secuencias de imágenes** (el punto (c)): no hay descodificador de vídeo (S33 lo deja para más adelante:
  AV1/VP9 por CPU). No se ha hecho la secuencia de PNG: cada cuadro de 1920 × 1080 cuesta ~100 ms solo en leerlo.
- **La GPU**: cada animado da su `Shader` (WGSL, con `tiempo`, `medida` y sus parámetros) para cuando `gpu-diseno`
  tenga cómo pintarlo; nada lo usa todavía.
- **Pausa por franja**: el aviso se retiene por superficie; una pantalla tiene dos franjas a 1920 × 1080, y si una se ve
  (la de arriba, bajo la barra translúcida) el animado sigue pintando las dos. Y solo cuentan como tapa los búferes sin
  alfa y las ventanas maximizadas: las ventanas del kit son ARGB (esquinas redondas); con `set_opaque_region` (que el
  compositor no guarda aún) se haría bien.
- Los textos de la herramienta no están en el catálogo (solo la fila de Ajustes y el error de PNG).
- Miniaturas sin caché en el disco (se rehacen al abrir; una foto 4K tarda segundos en QEMU).
- Un solo búfer por franja: una ventana que se mueve encima en el momento de pintar un cuadro podría pillarlo a medias
  en lo poco que cambia.

## Lo que no se ha comprobado

- Varias pantallas de verdad (`make arranque-virt-gpu` con dos cabezas): el código las lleva (una salida por
  `wl_output`, fondo por pantalla), no se ha visto en QEMU.
- El pase automático (cada N minutos) y `Fondo SIGUIENTE`: solo el código; ninguna prueba espera minutos.
- El cambio claro/oscuro con un fondo distinto para cada modo, en QEMU (sí en las pruebas de PC de los ajustes).
- Que el tema salga del fondo nuevo al aplicar desde la herramienta (en el perfil de shiva el acento es monocromo).
- Frenar con la máquina ocupada (el ritmo lo hace y lo dice; ninguna prueba carga la máquina a propósito).
- La Pi: nada.
