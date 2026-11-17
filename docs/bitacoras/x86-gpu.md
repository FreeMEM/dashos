# Bitácora: la pantalla de x86 sin parpadeo (rama `x86-gpu`)

Rama `x86-gpu`, desde `x86` (cade3fea). Lo pidió el usuario: en `make -C kernel x86-uso` (q35, KVM, ventana GTK) la
pantalla parpadea; en sus palabras, "no es haciendo algo concreto, por ejemplo abro una ventana y de repente parpadea
todo". En ARM (`virt`, con virtio-gpu) no pasa.

## 2026-11-16/17: qué se midió antes de tocar nada

La sospecha era que el compositor copia a la pantalla del GOP (un solo búfer, el que deja el firmware) píxel a píxel
con `write_volatile`, y QEMU refresca su ventana leyendo esa memoria cuando quiere: pillaría copias a medias.

- **Lo que se repinta entero.** Medido en el compositor (cada cuadro con más de un cuarto de pantalla dañado, quién lo
  pidió y lo que tardó; código de diagnóstico, ya quitado): después de entrar, la pantalla entera se recompone **una
  vez por cada programa que se lanza**. Es el aviso de `sesiones` de a quién pertenece el programa nuevo
  (`AVISO_PERSONA`): la escena recomponía todo tras cualquier aviso de sesiones. Con un resumen por fila del marco antes
  y después, esos repintados enteros dejan la imagen **igual** (0 filas cambian, en 12 de 13; la otra, el lanzador que
  se cierra). Lo demás que pasa del cuarto de pantalla son arrastres de ventanas y el lanzador.
- **Lo que tarda la copia.** Con KVM (AMD, este PC) la pantalla del GOP de la VGA de QEMU es RAM del anfitrión y el
  kernel la proyectaba como memoria normal (WB): la pantalla entera de 1280x720 (3,7 MB), píxel a píxel, 0,57 a 0,65 ms
  (lo mejor de 20); a 1920x1080, unos 5 ms con la máquina cargada. No es lenta en QEMU. En un PC de verdad los MTRR del
  firmware dejan la pantalla sin caché (UC): ahí cada píxel es una escritura suelta por el bus (no medido: no hay PC).
- **Lo que se ve.** `tools/x86_parpadeo.py`: entra Ana y, durante 60 s, abre el Shell desde el lanzador y lo arrastra,
  el panel rápido, el desplegable de procesos y el ratón de un lado a otro, mientras mira cuadros seguidos. Un cuadro
  "pasajero" es el que difiere del anterior y del siguiente cuando esos dos son iguales (algo que sale y se va en un
  cuadro); más del 2 % de la pantalla pasajero cuenta como parpadeo y se guarda. Dos fuentes de cuadros:
  - por QMP (`screendump`, lo que QEMU tiene en la pantalla en ese momento; 10 a 25 por segundo);
  - la ventana GTK de verdad, en un Xvfb propio, grabada con ffmpeg a 60 cuadros por segundo sin pérdidas (`VIDEO=`),
    con la ventana a tamaño real (`GTK_AJUSTAR=off`).

| Cómo | Pantalla | Cuadros | Parpadeos (>2 %) | Lo más pasajero |
|---|---|---|---|---|
| QMP, antes | 1280x720, GOP | 1471 | 2 | 8,4 % |
| QMP, antes | 1920x1080, GOP | 953 | 0 | 1,1 % |
| GTK grabado, antes | 1920x1080, GOP | 3723 | 0 | 0,35 % |
| GTK grabado, después | 1920x1080, GOP (`GPU_X86=vga`) | 3723 | 0 | 0,37 % |
| GTK grabado, después | 1920x1080, virtio-gpu | 3721 | 0 | 0,18 % |
| QMP, después | 1920x1080, virtio-gpu | 718 | 0 | 0,60 % |
| QMP, después | 1920x1080, GOP (`GPU_X86=vga`) | 759 | 0 | 0,65 % |

  Los dos parpadeos de la primera fila son **el lanzador de la barra pintado a medias**: el compositor compuso mientras
  la barra pintaba en su búfer (un solo búfer por superficie, que el cliente reescribe). Es del cliente, no de la copia
  a la pantalla (pendiente, abajo). Lo de hasta el 1 % son las ventanas que se arrastran (se mueven de un cuadro a
  otro: el método las cuenta).

**La causa no queda comprobada.** Ni por QMP ni grabando la ventana GTK (en X, no en Wayland como la del usuario) sale
la pantalla entera parpadeando. Lo que sí sale: cada programa que se abre repinta y copia la pantalla entera sin que
cambie nada (lo que mejor casa con "abro una ventana y parpadea todo"), y la copia al GOP no tiene un momento atómico:
quien la lee mientras se copia ve la mitad. Se arreglan las dos cosas y, en QEMU, se deja el GOP por la virtio-gpu, que
solo enseña lo que se le manda vaciar.

## Lo hecho

- **Sin repintados enteros de balde** (`wayland/src/escena.rs`): tras un aviso de sesiones se recompone todo solo si
  cambia quién no se ve (los clientes de una sesión de detrás); si no, solo lo del teclado y el puntero. Prueba en
  `sesion_de_detras` (falla con lo de antes).
- **La copia al GOP** (`usuario/src/bin/compositor.rs`, `presentar`): de arriba abajo por filas de pantalla, con todos
  los daños de cada fila, y cada trozo de fila de una vez (`copy_nonoverlapping`), sin cambiar los colores salvo en las
  pantallas que van en R, G, B. Así, lo que se pille a medias es la parte de arriba nueva y la de abajo vieja, nunca una
  ventana que dejó su sitio y aún no está en el nuevo (antes se copiaba daño a daño: primero el sitio viejo, después el
  nuevo). Al acabar, `dashos::barrera::pantalla()` (`sfence` en x86). La medida de cada 10 s dice ahora también lo que
  tardó la copia y a cuántos MB/s.
- **La pantalla del GOP con combinación de escritura (WC)** (`kernel/src/base/x86_64/cpu.rs` y `espacio.rs`): la entrada
  1 de la PAT (solo PWT, que nadie usaba) pasa de WT a WC en todos los núcleos, y `proyectar_firmware` proyecta la
  pantalla con ella (`Permiso::Pantalla`). Con los MTRR del firmware en UC, WC gana (SDM, tabla 13-7): las escrituras
  seguidas salen en ráfagas. Sin PAT, como antes.
- **virtio-gpu en x86** (`kernel/src/inicio_x86.rs`, `inicio_x86_uso.rs`, `usuario/src/bin/virtio_gpu.rs`): el
  controlador de ARM, el mismo. Las barreras, de `dashos::barrera` (llevaba `dmb` a mano), y la BAR de sus registros la
  que diga su capacidad común de virtio (la 4 en `virtio-gpu-pci`, la 2 en `virtio-vga`), en el kernel y en el
  controlador. En la imagen de usar, al final de la lista (como en `inicio.rs`).
- **El tamaño de la cabeza**: con la ventana GTK, QEMU dice a la virtio-gpu el tamaño de la ventana (visto en Xvfb:
  640x480 con `zoom-to-fit`, 1280x800 sin él), y la pantalla habría salido de lo que midiera la ventana al arrancar.
  El controlador recibe ahora "pantalla-info" (las medidas del GOP, como el compositor; por nombre, con
  `dashos::llaves`) y su cabeza 0 se queda con el modo del firmware (el de `PANTALLA`); la ventana lo escala, como
  antes. En ARM no se le da y sigue con lo que dice el aparato.
- **El compositor moría al pasar a la virtio-gpu a 1920x1080**: pedía el marco nuevo (8 MB) con el viejo aún vivo y
  topaba con su tope de memoria (16 MB). Ahora suelta el viejo antes (o lo reusa si mide lo mismo). No se veía en ARM
  porque allí las cabezas son pequeñas.
- **`make -C kernel x86-uso` lo usa por omisión** (`tools/qemu_x86_a_mano.sh`, `GPU_X86=virtio`): `-vga none -device
  virtio-vga,xres,yres` del tamaño de `PANTALLA`. Es la virtio-gpu de PCI con la VGA delante: OVMF da el GOP por la VGA
  (el cargador y el compositor arrancan con él, como siempre) y, cuando llega la cabeza del controlador, el compositor
  pasa a ella y QEMU enseña lo de la virtio-gpu. Sin virtio-gpu (o `GPU_X86=vga`, la VGA de QEMU sola) sigue el GOP: la
  reserva. `GTK_AJUSTAR=off` deja la ventana GTK del tamaño de la pantalla.

Por qué `virtio-vga` y no `virtio-gpu-pci` a secas: sin VGA, el GOP de OVMF para la virtio-gpu es solo de "Blt", sin
memoria de pantalla, y el compositor no arranca sin la pantalla del GOP (su llave no es opcional en la lista de
`inicio`). Que el compositor arranque sin pantalla y espere a las cabezas del controlador queda pendiente (abajo).

## Medidas

La pantalla entera de 1280x720 copiada 20 veces seguidas al arrancar (medida temporal en el compositor, ya quitada),
con KVM, la VGA de QEMU (`GPU_X86=vga`); lo mejor y la media, dos arranques cada caso:

| Proyección del GOP | Píxel a píxel (antes) | Por filas (ahora) |
|---|---|---|
| WB (antes) | 566 y 651 µs (media 878 a 885) | 73 y 86 µs (media 105) |
| WC (ahora) | 566 y 608 µs (media 716 a 899) | 221 y 227 µs (media 318 a 331) |

En QEMU con KVM la pantalla es RAM del anfitrión y WB es lo más rápido (la caché); WC va a unos 16 GB/s, el triple de
rápido que el píxel a píxel de antes. Se deja WC porque lo que importa es un PC de verdad, donde la pantalla es memoria
de la tarjeta, los MTRR la dejan en UC y WB no la haría más rápida (UC gana a WB). En QEMU, de serie, va la virtio-gpu.
En uso (la medida de cada 10 s), la copia a la pantalla: con el GOP, de 2,5 a 9 GB/s; con la virtio-gpu, de 3,4 a
9 GB/s (es memoria normal; lo que tarda el anfitrión en copiarla va en el controlador).

## Pruebas

Con todo el trabajo junto (no commit a commit), en `kernel/`: `make prueba` (con `cargo test` de `wayland`, que lleva
la prueba nueva), `make arranque-virt`, `arranque-virt-2` (multimonitor por virtio-gpu en ARM), `arranque-virt-linux`,
`prueba-x86` con y sin `ACCEL=kvm` (con KVM pasó, también C28), y `arranque-x86` con y sin `RED_X86=e1000e` y con y sin
`ACCEL=kvm`: todo bien. `arranque-x86` (TCG, virtio-net) falló dos veces solo en el sonido (chasquidos y huecos, con la
máquina a carga 26) y pasó a la tercera; la rama no toca el sonido. Además, `make x86-uso` sin ventana con
`tools/x86_uso_capturas.py`, con `GPU_X86=virtio` y `vga`: las 13 capturas como antes (cambian la hora y los números
del monitor).

## Lo que queda, lo pendiente y lo que NO se ha comprobado

- **Nada en un PC de verdad.** Ni la WC de la PAT con los MTRR de un firmware real, ni cuánto gana la copia, ni si hay
  desgarro visible con el GOP (sin doble búfer ni sincronía vertical en el GOP no se puede evitar del todo; con la
  copia rápida dura poco).
- **El parpadeo que vio el usuario no se ha reproducido** con las herramientas de arriba. Hay que mirarlo con sus ojos
  en su ventana GTK (Wayland) con `make -C kernel x86-uso` (ya con la virtio-gpu); y, si sigue, con `GPU_X86=vga` para
  ver si es la VGA.
- **Clientes que pintan en el búfer que el compositor lee** (el lanzador de la barra, el panel del fondo y, en general,
  los que reescriben su único búfer): si el compositor compone mientras pintan, sale a medias (visto con el lanzador).
  Lo propio de Wayland es pintar en otro búfer hasta que el compositor suelte el anterior (`wl_buffer.release`). Es de
  los clientes, no de esta rama: pendiente.
- **Alias de tipos de memoria**: el mapa lineal del kernel cubre los huecos de aparatos por debajo de 4 GB como memoria
  normal (los MTRR los dejan en UC); la pantalla queda así con un alias UC en el kernel (que no la toca) y otro WC en el
  compositor. Es lo mismo que ya pasaba con los registros de los aparatos (alias UC y WB); Intel lo desaconseja.
- **`virtio-gpu-pci` sin VGA** no sirve aún (el compositor necesita el GOP para arrancar).
- La virtio-gpu de x86 no está en las pruebas automáticas (`arranque-x86` no la lleva): se ha visto en `x86-uso`, con
  las capturas de `tools/x86_uso_capturas.py` (iguales que antes salvo la hora y los números del monitor).
- En un árbol nuevo, `make x86-uso` falla al hacer el disco si no se ha compilado antes `archivos/herramientas`
  (`duenos`); el Makefile no lo pide (igual en ARM). No tocado.
- Capturas y vídeos, fuera de git: `~/recuperar/geosian-7d/x86-gpu-capturas/`.
