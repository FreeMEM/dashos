# Bitácora: la tecla pegada y lo que paraba `make arranque-virt` tras la unión (tecla-pegada)

Rama `tecla-pegada`, sacada de `revision` (f27af229). El encargo: en `make -C kernel arranque-virt` el paso del
explorador fallaba con su búsqueda "llena de a" (union-dms: "una tecla se queda pulsada"). Encontrar la causa de verdad,
cumplir la regla de cualquier escritorio (al quitarle el teclado a una ventana se le sueltan las teclas pulsadas, y una
tecla soltada nunca repite), y seguir con lo que salga después del explorador.

## 2026-11-17

### Lo que de verdad pasaba en el explorador: no era una tecla pegada

- En DashOS nadie repite teclas: ni el compositor (no manda `repeat_info` ni repite), ni el kit, ni el explorador, ni la
  caja Amiga (su `Evento::Tecla` siempre va con `repeticion: false`). Solo repiten por su cuenta los programas de Linux
  (GTK y Firefox, con su `wl_keyboard`).
- Las "a" son del guion. `tools/prueba_uefi.sh` manda de fondo, cada vuelta, un movimiento, un clic y una "a" (desde
  U6). Mientras dura el paso del explorador eso se para (`continue`); si un paso del explorador no sale en 20 s, o no
  encuentra dónde pulsar, el guion lo deja (`ex = 99`) y vuelve lo de fondo: las "a" caen en el explorador, que tiene el
  teclado, y su búsqueda crece una letra por vuelta (con un "arrastre" del compositor entre medias: los clics de fondo
  caen en la barra de la ventana). Así en las cinco salidas guardadas con "busca \"aaa" (union-dms, fusion-fondo,
  prueba1, trasfondo, caja-amiga-procesos): antes de la primera "a" siempre falla otro paso del explorador (el 12 en
  union-dms, el 5 en fusion-fondo, el arrastre "aquí no" del 36 en las otras). La "a" es la consecuencia, no la causa.
- Con la máquina de hoy, el paso del explorador pasó entero en las seis pasadas que llegaron a él (también la primera,
  sin ningún cambio en el sistema; la 6 no llegó, y de la 4 no guardé la salida): el fallo del paso 12 de union-dms no lo he vuelto a ver. Lo que sí estaba en union-dms y
  aquí también es el compositor al 95 % por el dock (abajo), que hace todo más lento; si fue eso lo que movió aquel clic
  no lo he podido comprobar.

### Lo que sí estaba mal en el teclado (y se arregla igual)

Aunque no fuera lo del explorador, la regla no se cumplía, y una persona lo vería:

1. **El compositor no soltaba nada al quitar el teclado** (`wayland/src/escena.rs`). Al cambiar de ventana (clic, una
   ventana nueva, otro espacio, bloquear, una capa que se va) solo mandaba `leave`. La ventana que perdía el teclado con
   una tecla bajada no recibía su soltar: un programa de Linux la repetía sin fin; en los de DashOS, `kit::teclado`
   cuenta las Mayúsculas por pulsar y soltar, y se quedaban puestas. Y el soltar le llegaba a la ventana nueva, que no
   vio la pulsación.
2. **Soltar con Super por medio se perdía**: la "1" pulsada llegaba a la ventana; si luego se pulsaba Super y se soltaba
   la "1", el atajo Super+número se tragaba el soltar (los atajos miraban también al soltar).
3. **Dos teclados se pisaban** (`usuario/src/hid.rs`): un solo "informe anterior" para todos; el informe de uno soltaba
   lo que tenía pulsado el otro. Y un teclado desenchufado o prestado a un dominio con una tecla bajada no la soltaba.
4. **La cola sin sitio podía tirar un soltar** (`hid.rs`): con la cola del compositor llena y sin movimientos que tirar,
   se tiraba el evento más viejo, fuera lo que fuera.
5. Un informe con una tecla repetida (posible en un aparato malo) daba dos soltar.

### El arreglo del teclado

- **Compositor** (`Escena`): sigue las teclas cuya pulsación entregó a quien tiene el teclado (`entregadas`, como mucho
  `MAX_ENTREGADAS` = 32). `soltar_teclado` (por donde pasan ahora `dar_teclado`, bloquear, cambiar de espacio o de
  sesión y la capa que se va de la pantalla) le manda, antes del `leave`, el soltar de cada una y los modificadores a 0.
  Un soltar solo llega a quien recibió la pulsación, pase lo que pase entre medias (atajo, commodity); los modificadores
  se dicen siempre a quien tiene el teclado, y la ventana que lo recibe con alguno pulsado lo sabe tras el `enter` (como
  pide Wayland). Soltar no dispara nada en el compositor (los atajos van al pulsar; Super sola sigue igual). Si la
  superficie o el cliente ya no están, lo entregado se olvida. Una tecla ya entregada (otro teclado) no se entrega dos
  veces, y sin sitio para seguirla no se entrega (llegaría su pulsación y no su soltar).
- **HID** (`usuario/src/hid_teclas.rs`, nuevo, sin dependencias, se prueba en el PC): `Teclados` compara cada informe
  con el anterior de su teclado (por número de aparato) y suelta lo de un teclado que se va (`revisar`, que el xHCI
  llama cada vuelta con los que siguen: ni desenchufados ni prestados). `Cola`: sin sitio, primero un movimiento del
  ratón; luego una pulsación con su soltar detrás (la tecla no llega entera); luego una pulsación sola (no llega; su
  soltar, el compositor no lo da a nadie). Un soltar no se tira mientras haya otra cosa.

### Lo que salió después del explorador

Con el explorador pasando, la tanda sigue y salieron tres cosas más:

1. **El dock se repintaba sin parar** (`usuario/src/bin/dock.rs`): unos 500 commits por segundo desde el paso del dock
   (tras traer una ventana y fijarla, al esconderse y salir el OSD de Bloq Mayús), y el compositor al 95 % ("en 10 s:
   5516 cuadros, 9561 ms componiendo") el resto de la pasada. Ya pasaba en union-dms ("4394 cuadros, 9615 ms" desde ese
   mismo paso). Lo medí poniendo en el compositor, solo para medir, los commits por cliente cada 10 s: "4837 de 27
   (dock)". La causa: el dock contaba como animación cualquier salto en su lista (`!saltos.is_empty()`) y repintaba si
   había alguno (`saltos_antes > 0`), aunque el icono estuviera arriba y quieto; cada commit le trae un aviso del
   compositor (el búfer suelto) que lo despierta, y vuelta a empezar. Ahora `Vista::saltando` (solo los saltos que se
   mueven) decide la animación y el repintado; y al esconderse, el icono que estaba arriba baja (escondido no hay ratón
   encima). Con el arreglo: ver "Pasadas".
2. **El selector se quedaba sin ventana para `guarda`**: "[compositor] error de protocolo de selector: número de objeto
   nuevo malo" y "[selector] el usuario no guarda nada" sin que saliera la ventana. El selector abre una conexión nueva
   por ventana y se despide con `TROZO_ADIOS` sin mirar si se pudo escribir. Si el adiós no llega (con el compositor al
   95 %, la cola de "ventanas" llena), el `TROZO_CONECTAR` siguiente de la misma marca se ignoraba y sus datos iban a
   la conexión de antes, que ya tenía esos números de objeto. Ahora una marca lleva una conexión: conectar otra vez
   cierra la de antes (y se dice). Que el adiós se perdiera por la cola llena es lo que encaja, no lo he visto.
3. **"FALTA: [dash-ventana] pulsado"** (y, en otra pasada, **"FALTA: [centro] modo claro (lo pide el panel)"**): los tres clics del guion en la ventana de Dash cayeron en las tandas de
   administrador que el guion del Shell hace a la vez (`guardian` las pone encima, con el teclado). El paso de Dash ya
   esperaba a las preguntas de `guardian`, no a sus ventanas del sistema. Ahora espera a que no haya ninguna (tanda,
   código de recuperación, cambio de contraseña; cada una dice cuándo sale y cuándo se va) y 4 s más, y un intento en
   el que salió una no cuenta. El paso del panel rápido tenía el mismo hueco (la tanda de la cuenta Dora salió justo al
   abrirlo y se llevó el clic del mosaico del modo oscuro): espera igual, y si salió una ventana de `guardian` tras su
   último clic y no llegó lo esperado, lo repite.
4. **Disco tras la prueba** (`tools/disco_archivos.py --comprobar`, que tras la unión nadie había visto): la fecha de
   `Escrito/hola.txt` se pedía de los últimos 600 s y la pasada dura más (900 s de tope y el reinicio): ahora 1800. Y el
   registro común tiene un cuarto fichero, `dominio-red` (rama dominio-red), vacío si el dominio no apunta nada: se
   admite.
5. **La sesión se quedaba bloqueada**: el bloqueo por tiempo de `guardian` va a su ritmo y una vez cayó entre los dos
   cambios de contraseña del guion del Shell (Ana tenía "otra de Ana"); el guion tecleaba "clave de Ana" y no
   desbloqueaba (dos "contraseña mala"). Ahora teclea la que vale en ese momento (cuenta los "cambiada").

### Pruebas

- `cargo test` de `wayland`: 147, bien. Las nuevas (`tests_teclas.rs`), en el PC y hostiles: cambiar de ventana con la
  tecla bajada, modificadores al cambiar, una ventana nueva del sistema entre pulsar y soltar, otro espacio con la tecla
  bajada, soltar con Super por medio, soltar perdido y de más, la ventana o el programa que se van con teclas bajadas, y
  ráfagas al azar (40 × 400 pasos: pulsar, soltar, soltar de más, Super con números y flechas, clics, ventanas nuevas
  del sistema, ventanas que se cierran, programas que se van, otro espacio) contra un modelo de lo que cree cada
  cliente. Comprobado que fallan las ocho con el compositor de antes.
- `hid_teclas` (7) y `dock_modelo` (29, con el saltito quieto), en el PC; `hid_teclas` va en `make prueba`.

### Pasadas

Con otras sesiones probando a la vez (carga de 4 a 9 en 32 hilos).

| Pasada | Código | Resultado |
|---|---|---|
| 1 | la base (solo `GUION_TRAZA`) | explorador bien; falla `guarda` (selector, "número de objeto nuevo malo"); el compositor al 95 % desde el dock |
| 2 | teclas, HID y reconectar (sin el dock) | explorador bien; "FALTA: [dash-ventana] pulsado" (los clics en las tandas del Shell) |
| 3 | más el dock y el paso de Dash | BIEN las dos listas de líneas y el reinicio; falla la comprobación del disco (fecha de 600 s, `dominio-red`): arreglada y comprobada sobre ese disco |
| 4 | todo | "FALTA: [centro] modo claro (lo pide el panel)" (el clic del panel en una tanda) |
| 5 | más el panel | BIEN entera |
| 6 | igual | sesión bloqueada (la contraseña cambiada a medias por el Shell) |
| 7, 8 | más la contraseña | BIEN enteras |

Además, con todo menos el arreglo de la contraseña (que solo toca ese paso del guion de `arranque-virt`): `make
arranque-virt-wayland`, `-shell`, `-menus` y `-2`, BIEN a la primera; `make arranque-virt-linux` BIEN, con todo.
`make prueba`: BIEN.

El compositor tras el arreglo del dock: el máximo en 10 s, de 288 a 424 cuadros en las pasadas buenas (antes, de 4000 a
6800 seguidos desde el paso del dock hasta el final). Ninguna pasada volvió a sacar "busca \"aa", ni "conecta otra vez"
(el adiós del selector llegó siempre: sin el compositor al 95 %, la cola no se llena).

**Lección**: no editar `tools/prueba_uefi.sh` mientras corre una prueba que lo usa. Bash lee el guion a trozos según
avanza: lo cambiado mueve lo que le queda por leer. Paré dos veces la tanda por eso y la repetí.

### Pendiente del usuario

- Nada nuevo que cambie lo que se ve, salvo un detalle del dock: al esconderse, el icono que estaba subido baja (antes
  se quedaba arriba y volvía a salir subido sin el ratón encima).

### Lo que queda

- `GUION_TRAZA=fichero` (nuevo en `prueba_uefi.sh`): cada envío del guion, con la hora y el largo de la salida serie en
  ese momento. Así se puede poner lo que hizo el guion junto a lo que dijo el sistema; con esto vi el paso de Dash.
- Cuando un paso del explorador se rinde, el guion vuelve a teclear "a" de fondo y llena su búsqueda: la línea que falta
  lo dice bien, pero la salida engaña (como engañó en union-dms). No lo he cambiado.
- El selector sigue sin mirar si su `TROZO_ADIOS` se escribió (con el arreglo del compositor ya no hace daño).
