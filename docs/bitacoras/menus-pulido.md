# Los menús: lo que quedó (rama `menus-pulido`)

Rama `menus-pulido`, desde `revision` (d45a8281, con los menús unidos). Sigue lo que dejó `docs/bitacoras/menus.md`
("Lo que queda") y la decisión del usuario de S10 ("Dónde van los menús de un programa"). Encargo:

1. Con un menú abierto en la barra, un clic en una ventana lo cerraba y además le llegaba a la ventana. Debe solo
   cerrarlo.
2. Probar en QEMU las teclas dentro de un submenú abierto en la barra y la rueda en un menú que no cabe.
3. Lo mismo dentro de la ventana (kit nativo y kit-c).

## Diseño en tres frases

El clic que cierra lo decide el compositor, que es el único que sabe dónde cae un clic antes de dárselo a nadie: la
barra le dice cuándo tiene algo desplegado (`dashos_menus_barra_v1.desplegado`), y mientras tanto el primer botón que
se pulsa fuera de las superficies de la barra no llega a nadie (ni él ni su soltar, ni sube la ventana ni le da el
teclado) y la barra recibe `cerrar`. Un clic, uno solo: tras él el compositor da lo desplegado por cerrado aunque la
barra no conteste, y si la barra se va o destruye su objeto, deja de tragar. Lo de las teclas y la rueda en lo que no
cabe estaba a medias en el componente del kit (el submenú no se desplazaba nunca) y en la barra (la rueda en el
submenú usaba el sitio de la capa del menú); se arregla en `kit::menus` y en `barra_menus`, y se prueba en QEMU con
un programa nuevo del kit nativo (`menus-prueba`) y con un menú largo en la prueba de Dash.

## 1. El clic que cierra (`wayland/src/menus_escena.rs`, `escena.rs`, `barra_menus.rs`)

Antes: la capa de lo desplegado es del tamaño justo; un clic fuera caía en la ventana de debajo, que lo recibía, se
subía y se llevaba el teclado; la barra cerraba el menú al perder el teclado. Arreglarlo en la barra no se puede: la
barra se entera después, cuando la ventana ya tiene el clic.

- Protocolo: `dashos_menus_barra_v1.desplegado(abierto)` (petición, solo de quien tiene el objeto de barra) y
  `dashos_menus_barra_v1.cerrar` (evento). `tools/wayland_tablas.py` regenera `tablas.rs` (solo esas dos líneas).
- Compositor: `menus_tragar`, al principio de `raton` (con el cursor ya movido): un botón que se pulsa con algo
  desplegado y fuera de las superficies del programa de esa barra se quita de lo que ve el resto de `raton` (y su
  soltar), se avisa `Cerrar` a la barra y se apunta en el registro ("un clic fuera cierra lo desplegado en la barra:
  no llega a nadie"). Va antes de la barra de DashOS de las ventanas, de sus bordes y de Super+arrastrar: un clic en
  el título de otra ventana tampoco la sube. Con la sesión bloqueada no traga nada. El botón derecho fuera también
  solo cierra (antes, sobre una ventana con menú, cerraba por `abrir`; el resultado es el mismo).
- Barra: dice `desplegado` cada vez que la capa del menú aparece o se va (en `sincronizar`, el único sitio que crea y
  quita capas) y atiende `cerrar`. Lo de cerrar al perder el teclado se queda, por si acaso.

Pruebas (PC, `wayland`): `el_clic_que_cierra_no_llega_a_la_ventana` (la activa, la de detrás sin subirla ni darle el
teclado, el botón derecho, el escritorio vacío, la barra que lo cierra ella), `el_clic_en_la_barra_le_llega` (otro
título o lo desplegado: es de la barra) y la hostil `hostil_el_clic_que_cierra` (cien vueltas abriendo y cerrando con
tres botones; dos botones a la vez; pulsar antes de abrir y soltar después; la barra que se va o destruye su objeto
con algo desplegado; un programa sin objeto de barra que lo intenta: error de protocolo; bloqueada). Sin el arreglo,
las tres fallan (comprobado quitando la llamada).

## 2. Las teclas y la rueda en lo que no cabe (`kit/src/menus.rs`, `barra_menus.rs`)

Lo que fallaba, visto en el código y confirmado con su prueba:

- El submenú no tenía desplazamiento: con el teclado, bajando por uno que no cabe, el foco se iba por debajo sin que
  se viera (y con Intro se elegía algo invisible); la rueda encima de él no hacía nada. Ahora `ver_sub` lo desplaza
  para que se vea la actual (como `ver` en el menú), y la rueda encima del submenú abierto lo desplaza a él (encima
  del menú, al menú, como antes). Lo común, en `primera_para_ver` y `desplazar`.
- En la barra, la rueda sobre la capa del submenú sumaba el sitio de la capa del menú: caía en otro sitio. Ahora
  `rueda` recibe la superficie y usa la suya.
- Un giro rápido llega como un solo evento de varios pasos (el ratón USB de QEMU junta así los que llegan seguidos) y
  la rueda solo bajaba una cosa (`signum`). Ahora baja una por paso, con los topes. Lo destapó QEMU (cinco pasos
  bajaban cuatro).
- La rueda no cambiaba la actual: la que estaba bajo el ratón antes de desplazar seguía marcada, y como la barra
  vuelve a colocar los menús con cada repintado (el reloj), `colocar` desplazaba otra vez hasta ella: el menú subía
  solo una cosa. Ahora la actual pasa a ser la que queda bajo el ratón. También lo destapó QEMU (de la 4.ª volvía a
  la 3.ª, siempre en el mismo sitio).
- Para las pruebas en QEMU: `Menus::lo_que_se_ve` (el menú abierto, de qué cosa a qué cosa se ve y cuál tiene el
  foco; y su submenú), que la barra (`[barra] se ve:`), kit-c (`[dash-ui] se ve:`) y `menus-prueba` dicen por la
  consola cuando cambia; y la barra dice también dónde está cada submenú y, al abrirse uno, sus entradas
  (`[barra] menú abierto: Ver > Muchas; ...`).

Pruebas (PC, `kit`): `submenu_que_no_cabe` (32 entradas en 300 de alto: bajando, la actual se ve siempre; Fin,
Inicio; la rueda sobre el submenú lo desplaza a él y no al menú; un evento de tres pasos baja tres, uno de `i32::MAX`
se para en el tope; ráfagas de cien pasos en cada sentido se paran en los topes; un clic elige lo que quedó debajo)
y, en `hostil_menus_que_no_caben`, la rueda con el ratón encima seguida de `colocar` (no vuelve atrás). Sin
`ver_sub`, o sin poner la actual en la rueda, fallan (comprobado quitándolos).

## 3. Dentro de la ventana

- El kit (`Interfaz`) ya pasaba las teclas y la rueda a los menús de la fila; con lo de arriba, el submenú también.
- Nuevo: si el teclado se va a otra ventana con un menú de la fila abierto, se cierra (`Interfaz::teclado_sale`, en
  kit-c, en `menus-prueba` y en el Shell). Antes se quedaba abierto en una ventana de detrás.
- kit-c y la prueba de Dash: Dash 2.0 no deja escribir C en los programas (`@native` es solo de `lib/amiga`) y su
  runtime no tiene aún `mui_*` para los submenús, así que la prueba de Dash no puede tener un submenú (lo de pedir a
  la sesión de Dash sigue en `menus.md`). Lleva ahora un menú "Largo" de 30 entradas, que no cabe en su ventana.

## El programa de prueba `menus-prueba` (`usuario/src/bin/menus_prueba.rs`)

Del kit nativo (`Interfaz`), solo en la imagen de los menús (característica `menus` del kernel; una línea en
`kernel/src/inicio.rs`). Menús: "Ver" (una entrada, una casilla, el submenú de opciones "Ordenar por", el submenú
"Muchas" de 32 entradas, "Cambiar el menú 20 veces" y "Cerrar la ventana en 3 s") y "Largo" (40). Abre su ventana la
segunda vez que los menús van a la barra (lo de antes del guion ya está hecho y no lo tapa). Es casi tan ancha como la
pantalla (780): con el Shell delante, su borde derecho se ve y el guion le da el teclado con un clic.

## El guion (`tools/guion_menus.py`, `make arranque-virt-menus`)

Pasos 7 a 11 nuevos (lo de antes, igual): teclas en la fila del Shell; teclas, rueda y ráfagas en la ventana de
Dash; el menú de la fila que se cierra al perder el teclado; `menus-prueba` en la barra (teclas en el submenú que no
cabe, Esc que cierra solo el submenú, la rueda en "Largo" y en "Muchas", el clic que cierra, ráfagas y su menú
cambiado veinte veces con lo desplegado abierto, sin cambiar de generación); dentro (teclas en el submenú y la rueda
en "Largo"); y la ventana que se cierra con "Largo" desplegado en la barra. Cada comprobación que no sale para el
guion (`FALLA: ...` en `menus.guion.txt`) y la prueba falla: sin `MenusHechos`. Capturas 14 a 24, en
`~/recuperar/geosian-7d/menus-pulido-capturas/`. El tiempo de la prueba pasa de 400 a 900 s (se para en cuanto salen
todas las líneas).

## Pruebas

Sobre lo último, con la máquina cargada por otras pruebas:

- `cargo test` de `wayland` (132, 1 ignorada) y de `kit` (71, 1 ignorada): bien.
- `make arranque-virt-menus`: bien en la quinta vuelta. Las anteriores destaparon: un fallo del guion (de camino a la
  esquina para la captura, el ratón pasaba por otro título y abría ese menú); los dos de la rueda de arriba; y que
  la ventana de prueba quedaba tapada entera por el Shell (de ahí que sea ancha). En la quinta, la rueda dio los pasos
  justos en los tres sitios y el guion llegó al final (`acabado`).
- `make prueba`, `arranque-virt`, `arranque-virt-2`, `arranque-virt-linux`, `arranque-virt-shell` y
  `arranque-virt-wayland`: bien, a la primera.
- No se midió rendimiento: lo que añade el compositor a cada evento del ratón es mirar un `Option` y, solo con algo
  desplegado y un botón nuevo, qué superficie hay debajo.

## Decisiones pendientes del usuario

Ninguna de las de `menus.md` se ha tocado (aspecto, Ctrl para los atajos, botón derecho al pulsar, nombres). Lo que
se eligió aquí por ser lo de cualquier escritorio, y queda apuntado:

1. El clic que cierra lo desplegado en la barra se traga cualquier botón (también el derecho y el central) y en
   cualquier sitio fuera de la barra: una ventana, su título, su borde o el escritorio vacío. No sube la ventana ni
   le da el teclado. Solo el primero: el siguiente llega como siempre.
2. Un menú abierto en la fila de una ventana se cierra si el teclado se va a otra (kit, kit-c y el Shell).
3. La rueda baja una cosa por paso del ratón (antes, una por evento, aunque el evento trajera varios pasos).

## Lo que queda

- Dash: con `@native` cerrado en Dash 2.0 y sin `mui_*` de submenús en su runtime, la prueba de Dash no puede tener
  un submenú; el submenú dentro de una ventana de kit-c solo se prueba por el kit (es el mismo componente y la misma
  `Interfaz`). Pedir a la sesión de Dash lo que ya decía `menus.md`.
- La rueda en un submenú **dentro** de una ventana se prueba en el PC (kit), no en QEMU (en la barra sí).
- Lo demás que la barra despliega (el lanzador, los procesos, el historial de avisos) sigue con lo de antes: un clic
  fuera llega a la ventana y lo cierra al perder el teclado. Si se quiere igual que los menús, basta con que la barra
  diga `desplegado` también para ellos (el nombre del protocolo es de los menús; habría que pensarlo).
- Los emergentes de los programas de Linux (`xdg_popup` con agarre) siguen como estaban: el clic de fuera los cierra
  y también llega (lo de `emergentes.rs`, sin tocar).
