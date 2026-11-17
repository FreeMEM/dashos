# Bitácora: el paso de Firefox de `arranque-virt-wayland`, firme con la máquina cargada (firefox-firme)

Rama `firefox-firme`, sacada de `revision` (ebb99cf). El encargo: `make -C kernel arranque-virt-wayland` fallaba a menudo
en la tanda completa (2 de 3 hoy) y casi nunca sola, con "FALTA: título: Linux · Index of file:///usr/local/share/dashos/";
y después del fallo seguía hasta su tope (4500 s). Una vez la barra de direcciones de Firefox recibió "are/dashos/"
(`docs/REVISION_LOOP.md`, el aviso de memoria a las cajas).

## 2026-11-16: la causa, el arreglo y la parada temprana

### La causa: una ventana nueva se lleva el teclado entre el clic y lo tecleado

Pasada con carga sin tocar nada (A1, más abajo): falló, pero en otro sitio, "FALTA: ventana_wayland: tecla 30
pulsada". La salida lo dice todo:

```
│   [caja-linux] linux: ventana_wayland: clic del botón 272 en (160, 120)
│   [barra] título: 68K · Hola Dash
│   [caja-linux] linux: ventana_wayland: tengo el teclado
│   ...
│   [caja-linux] linux: ventana_wayland: 0 teclas y 1 clics; adiós
```

El guion pulsa en la ventana, espera a que el programa diga que le llegó el clic y teclea "a". En medio salió una
ventana de la caja Amiga ("Hola Dash", de su tanda de demos) y se llevó el teclado: la "a" fue a ella. La regla de S8
(`Escena::puede_tomar_teclado`) deja a una ventana nueva de otro programa llevarse el teclado si no se ha pulsado
una tecla en `SIN_ROBAR_MS` (2 s), y un clic no contaba. Con la máquina cargada, las ventanas de la caja Amiga salen
cuando les toca a ellas y caen encima de los pasos del guion de Linux; sola, casi nunca coinciden. Hasta en la pasada
buena de la tanda de hoy (la salida de `prueba3` de las 17:36) la de "Hola Dash" salió justo después de la "a".

En Firefox es lo mismo, con más hueco: el guion pulsaba en la barra de direcciones y esperaba 2 s antes de teclear (sin
tecla en esos 2 s ni en los 20 de antes, la ventana nueva puede llevarse el teclado). Lo tecleado mientras la otra
ventana lo tiene se pierde para Firefox; si se cierra a medias (las demos de la Amiga se cierran solas), el teclado
vuelve a Firefox y le llega solo el final: "are/dashos/". No tengo la salida de los fallos de hoy (la de `prueba3` ya
era de otra pasada, buena); que la última línea fuera "[barra] título: Ajustes" encaja con otra ventana con el teclado
al final, pero no lo he podido ver.

Otra pasada sin el arreglo (B2) dio justo el fallo del encargo ("FALTA: título: Linux · Index of ...", y la última
línea de la barra, "título: Ajustes"), pero sin robo: la barra no nombró otra ventana entre el clic y lo tecleado, y la
captura enseña Firefox con hola.html y su barra de direcciones sin tocar. Firefox no se repintó nada tras el clic (el
compositor, a 10 cuadros cada 10 s, de unos 900 kpx: solo la barra), como si no le llegara nada o no atendiera. Lo
pasé dos veces más con `CAJA_FIREFOX_DEPURAR=1` (lo que recibe Firefox por Wayland) y no se repitió: **esta segunda
forma del fallo sigue sin explicar**. El guion nuevo no la tapa: sin otra ventana por medio, no repite y falla.

Descartado con medidas: que se perdieran teclas por el camino. Puse una medida provisional en `usuario/src/hid.rs`
(cuánto tiempo la cola del compositor no admite la entrada del xHCI y si se tira alguna) y la traza `hid_kbd_queue_full`
de QEMU (la cola del teclado USB de QEMU): en las pasadas con carga, ni una línea de las dos. El compositor sí tira
lo más viejo cuando le esperan más de 32 eventos sin sitio (ver "Qué queda"), pero no es lo que pasa aquí.

### El arreglo

1. **Probado y quitado: que un clic cuente como una tecla en el compositor** (`SIN_ROBAR_MS`, para que una ventana nueva
   no se lleve el teclado en los 2 s tras un clic). Tenía su prueba y pasaba la de Wayland, pero rompió
   `make arranque-virt` tres de tres: el guion de teclas hace clics cada medio segundo y cuenta con que la ventana del
   centro de control, al salir, se lleve el teclado (deja de teclear 3 s antes); con el cambio no se lo llevaba y el
   tramo del centro se quedaba parado ("[guardian] pide al centro abrir Permisos: 0, y se cerró: no"). Y es peor para
   una persona: quien abre un programa con un clic en el lanzador quiere escribir en su ventana, que sale en menos de
   2 s. Hacerlo bien es lo de los escritorios con un testigo de activación (quien lanza da un testigo y solo la ventana
   que lo trae puede llevarse el teclado tras un clic, como `xdg-activation`): queda apuntado, no hecho. El arreglo se
   queda en el guion, que es donde estaba el fallo: teclear sin dejar el hueco y comprobar a quién le llegó.
2. **El guion** (`tools/guion_caja_linux.py`, `teclear_en`): antes de teclear espera la señal de que la ventana tiene el
   teclado (la barra de DashOS nombra la ventana con el teclado: tiene que ser la última que nombró) y después la línea
   que dice que llegó. Si no llega y entre medias la barra nombró otra ventana, otra se llevó el teclado (puede pasar si
   la máquina tarda más de 2 s entre el clic y la primera tecla): se apunta en las medidas y se repite (hasta tres
   veces). Si no la nombró, no se repite: es un fallo de verdad. Lo usan la "a" de `ventana_wayland` y la dirección de
   Firefox. El hueco entre el clic y la primera tecla, por debajo de los 2 s: en `ventana_wayland`, la tecla justo tras
   el clic (antes se esperaba a que el programa dijera por la serie que le llegó el clic, que con la máquina lenta
   tardaba más); en Firefox, 1 s (antes, 2: no es para Firefox, lo tecleado le llega en orden tras el clic).
3. **Parada temprana** (`tools/prueba_uefi.sh`, con `CAJA_LINUX`): cuando acaba el guion de la caja, lo que falta ya no va
   a salir. Si faltan líneas `GRACIA_GUION` s después (300 de serie; 30 si el guion dejó dicho qué falló en
   `PREFIJO.fallo`), se para y dice qué faltó, con lo que dijo el guion. El guion escribe ahí con `fallo()` (Firefox sin
   ventana, Firefox sin hacer caso, la "a" que no llegó). Probado con un QEMU y un guion falsos: con `.fallo`, para a los
   30 s; sin él (gracia 8 s), a los 10 s; en los dos dice "FALTA" y la línea.
4. `usuario/src/hid.rs`: un evento de teclado tirado porque el compositor no recoge la entrada se dice en la salida (antes
   se tiraba callado).
5. **El desplegable de Ajustes en `make arranque-virt`** (`tools/prueba_uefi.sh`, guion de teclas): la misma familia. En
   `fusion-fondo`, con carga, "FALTA: [ajustes] el desplegable dice: Destello": el menú se abrió y, a la vez, salió una
   pregunta de `guardian` (las de la caja Amiga: "capa de guardian en (140, 186)"), que va encima y se lleva el teclado;
   el clic en la segunda opción no la eligió y el guion daba el menú por hecho. Ahora no abre el menú con una pregunta
   en pantalla (`libre_caja`), lo vuelve a abrir si sale una con el menú abierto, y si en 6 s Ajustes no dice el valor
   elegido, lo repite (hasta tres veces). El otro fallo de allí, "FALTA: [ajustes] el campo dice: DashOSa", no lo he
   podido ver (su salida ya estaba pisada): Ajustes solo dice el primer cambio del campo, así que otra letra que le
   llegue antes (una "s" para una pregunta que aún no tenía el teclado, por ejemplo) lo explicaría, pero no está
   comprobado.

6. **Dos pasos más del guion de teclas de `make arranque-virt`, de la misma familia**, que fallaron en mis pasadas con
   carga: el centro de control (el clic en la fila y en Quitar le llegó a una ventana de la caja Amiga que salió
   encima, "68K · Ajustes" y "ReAction Demo"; los reintentos eran flechas y Supr, que tampoco le llegaban, y el guion se
   quedaba ahí con `continue` mientras `guardian` bloqueaba la sesión: "[guardian] pide al centro abrir Permisos: 0, y
   se cerró: no", luego "FALTA: [dash-ventana] pulsado"), y Super+Arriba en Ajustes ("FALTA: [ajustes] el compositor
   pide 800x560": el atajo fue a la ventana Amiga que tenía el teclado). Ajustes repite el maximizar si en 8 s no lo dice
   (hasta tres). Para el centro hice un reintento con el ratón, pero lo quité (cbffd0b): la rama `cuentas-2`, ya en
   `fusion-fondo`, lo hace igual y mejor (lo trae delante con un clic en su borde antes de cada intento), y los dos
   chocaban al unir. Con el mío quitado, esta rama se une a `fusion-fondo` sin conflictos.

### Pasadas con carga

Dos QEMU míos a la vez (este worktree y otro mío, `firefox-firme-carga`, con el código de antes o el de después), más
las pruebas de otras sesiones; en algunas, además, procesos que gastan CPU (10 a 22; la carga, de 18 a 34 en 32 hilos).

| Prueba | Código | Pasadas | Bien | Lo que falló |
|---|---|---|---|---|
| arranque-virt-wayland | antes | 4 | 2 | la "a" de ventana_wayland a una ventana Amiga; Firefox sin hacer caso (B2) |
| arranque-virt-wayland | después (final) | 3 | 3 | — |
| arranque-virt | antes | 1 | 0 | el desplegable de Ajustes (pregunta de guardian encima) |
| arranque-virt | después (final) | 3 | 1 | el explorador (búsqueda llena de "a"); el selector de carpeta sin teclado (una ventana Amiga encima) y el campo de Ajustes "aDashOS" |

Además, `fusion-fondo` con esta rama unida (con los arreglos de `cuentas-2` para el centro), una pasada de
`arranque-virt` con carga: el centro y Ajustes, bien; falló "[dash-ventana] pulsado" (la ventana de Dash, que según
dms-barra tapa la de ReAction de la caja Amiga justo al hacer clic).

**Resumen: la causa común es una sola, y no son teclas perdidas por el camino.** Las ventanas de la caja Amiga (sus
demos) y las preguntas de `guardian` salen a una hora que depende de la carga. Cuando el guion deja más de 2 s sin
teclas, la regla S8 deja a una ventana nueva llevarse el teclado, y lo que el guion manda después (teclas, atajos,
clics donde se pone encima) le llega a ella. Los guiones suponían que la ventana a la que hablan está delante y con el
teclado. En la caja Linux ya no lo suponen (se mira lo que dice la barra antes y después); en el guion de teclas de
`arranque-virt` solo lo he hecho en Ajustes (desplegable y maximizar), y `cuentas-2` en el centro. Quedan pasos del
mismo guion con el mismo hueco: el explorador, el selector de carpeta, el campo de Ajustes y la ventana de Dash. Ir paso
a paso es lo que queda; la alternativa de raíz es la del compositor (abajo, pendiente del usuario).

Con versiones intermedias del arreglo hubo más pasadas (contadas aparte, no en la tabla): la de Wayland falló una vez
con 22 procesos de carga (la primera ventana de Firefox, a los 233 s; de ahí lo de los 150 s) y otra porque una orden
de la terminal se tecleó en una ventana Amiga (de ahí `a_la_terminal_si_no_la_tiene`); `arranque-virt`, con el
compositor cambiado, tres veces en el centro (de ahí quitarlo), y una en Super+Arriba (de ahí su reintento). El fallo
suelto conocido del explorador ("suelta en la ventana 1: mueve a Home:Privado": las "a" del guion acaban en su
búsqueda) salió dos veces con el código final; es de la misma familia y no lo he tocado.

### Pendiente del usuario

- Que una ventana nueva de otro programa no pueda llevarse el teclado salvo que la abra el usuario (un testigo de
  activación: quien lanza desde la barra o `sesiones` lo da, y solo esa ventana se lo lleva; las que un programa abre
  por su cuenta salen sin teclado). Es lo que arreglaría de raíz toda la familia, en las pruebas y para una persona
  (que no le roben lo que teclea). Cambia lo que se ve; sin decidir, no lo he hecho. Contar un clic como una tecla no
  vale: estropea abrir programas con el ratón (probado y quitado).

### Qué queda

- La forma del fallo de B2 (Firefox con el teclado y sin hacer caso), sin causa.
- `prueba-firefox-guion` (en la imagen de Arch) cierra Firefox a los 150 s pase lo que pase: con la máquina muy cargada
  (carga 34 en 32 hilos) su primera ventana tardó más y el guion la perdió. Ahora el guion lo dice ("la primera ventana
  no salió en los 150 s"), pero el plazo sigue en la imagen: cambiarlo es rehacerla (`preparar-arch`), y la tanda usa la
  misma caché.
- `usuario/src/hid.rs`: con la cola del compositor llena y más de 32 eventos esperando, se tira el más viejo (ahora se
  dice). Los botones del ratón cuentan como movimiento y se tiran callados. No pasó en ninguna pasada.
