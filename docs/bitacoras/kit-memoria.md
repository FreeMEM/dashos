# Bitácora: kit-memoria (en qué gastan memoria los programas del kit)

Rama `kit-memoria`, sacada de `revision` en 7cf5125 (el montón en trozos de 128 KB a 4 MB).

## 2026-11-12: medir, y tres cambios

### Lo medido

Las cifras de memoria son lo cobrado al grupo de cada proceso: la línea `[centro] memoria por proceso` de
`make arranque-virt` (QEMU virt, pantalla de 800 × 600). Para el montón, contadores nuevos en
`usuario/src/monton.rs` (`monton::uso()`: en uso, máximo, reservado y lo que espera en las listas por tamaño). Con
`monton::con_nombre`, el programa dice en la consola cada trozo que pide, con lo que había en uso y el pedido que lo
causó; lo hacen los ocho programas del kit (barra, centro, ajustes, panel, selector, guardian, workbench, fondo).

Reparto antes, por programa (MB cobrados; montón en uso al pedir el último trozo):

| Programa | Cobrado | Montón reservado | Montón en uso | Búferes de ventana |
|----------|---------|------------------|---------------|--------------------|
| barra | 6,4 | 4 | 2,04 | 2,1 (un pool para todo: la barra a 3840 de ancho, lanzador, aviso e historial) |
| centro | 5,5 | 4 | 2,04 | 0,9 (560 × 420) |
| ajustes | 4,9 | 4 | 2,04 | 0,45 (360 × 320) |
| panel | 4,9 | 4 | 2,04 | 0,47 (300 × 400) |
| selector | 4,5 | 4 | 2,04 | la ventana (460 de ancho) |
| guardian | 4,4 | 4, y luego 8 | 2,04; luego 2,35 | la pantalla, mientras se ve |
| workbench | 2,8 | 2, y luego 16 | 1,0; luego 12,3 | el escritorio y cada ventana |

Lo demás (imagen del programa, pila de 64 KB, proceso, canales de 26 KB) son 0,1-0,3 MB.

De dónde salen los 2 MB de montón: las dos fuentes. `fontdue` convierte en líneas, al cargar la fuente, los contornos
de todas sus letras (378 en Roboto recortada): medido en el PC con un asignador que cuenta, 1.050 KB por fuente, y
cada programa del kit carga Roboto normal y Medium. La caché de glifos rasterizados es poca cosa: 130 KB para 856
glifos en 6 tamaños. Con el montón doblando, 2,04 MB en uso eran 4 MB reservados. El workbench crea un `Contexto`
(dos fuentes) por cada ventana con interfaz: llegaba a 12 MB en uso. El guardián pintaba una pantalla de prueba de
800 × 600 en el montón (1,9 MB) para rasterizar los glifos antes de bloquear.

### Lo cambiado

1. **Texto sin `fontdue`** (`kit/src/texto.rs`). Cada letra se rasteriza la primera vez que hace falta, con su
   contorno leído de la fuente en ese momento con `ttf-parser` (el que ya usaba `fontdue`, sin crates nuevos). El
   rasterizador es propio: aplana las curvas en tramos (error de 0,05 píxeles como mucho) y acumula el área con signo,
   repartiendo cada trozo de tramo entre su celda y la de la derecha según su punto medio. Cargar una fuente ya no
   cuesta nada. Interletraje: el de la tabla `kern`, como `fontdue` (Roboto recortada no la trae: sigue sin él).
   Comparado en el PC con `fontdue`, 14 líneas de prueba (regular y Medium, de 11 a 32 píxeles): mismos anchos;
   tinta total igual en ±0,2 %; diferencia media de 0,2 a 1,3 sobre 255 por píxel con tinta; solo 5 píxeles de
   todos difieren en más de 64 (en 11 y 12 píxeles). Prueba nueva: `rasterizar_llena_por_dentro`.
2. **El montón crece la mitad de lo que tiene** (`usuario/src/monton.rs`), no lo mismo: 128, 128, 128, 192, 320...
   KB, en múltiplos de 64 KB, hasta 4 MB. Lo reservado pasa como mucho en un 50 % a lo que hace falta (antes, casi el
   doble). Un pedido mayor que el trozo que toca se pide de una vez: el fondo, con 300 KB en uso, pedía cinco trozos
   para una imagen de 1,7 MB.
3. **La barra pide la memoria de cada capa solo mientras se ve** (`Lamina`, en `usuario/src/ventana_cliente.rs`: la
   memoria, su proyección y su `wl_shm_pool`). La barra, del ancho que tiene (800 × 40 en QEMU); el lanzador, el aviso
   y el historial, al abrirse, y se sueltan al cerrarse. Antes eran 2,1 MB siempre.
4. **El guardián precalienta los glifos en 400 × 300** (0,47 MB), no en 800 × 600: las letras son iguales.
5. **Conectar al compositor reintenta con su cola llena** (-8), hasta un segundo. Con el texto nuevo los programas
   arrancan unos milisegundos antes, y en una pasada el panel dijo "sin compositor" y no estuvo. No volvió a pasar,
   pero tampoco se vio el -8: el porqué queda sin confirmar (la línea `[ventanas] no se puede conectar` lo dirá).

### Antes y después

Cobrado al grupo (MB), en `make arranque-virt`:

| Programa | Antes | Con 2 y 3 (aún `fontdue`) | Con todo |
|----------|-------|---------------------------|----------|
| barra | 6,4 | 3,6 | 0,7 |
| centro | 5,5 | 4,5 | 1,6 |
| ajustes | 4,9 | 3,9 | 1,0 |
| panel | 4,9 | 3,9 | 1,0 |
| selector | 4,5 | 3,5 | 0,6 |
| guardian | 4,4 | 3,5 | 0,6 |
| workbench | 2,8 | 2,1 | 0,9 |
| fondo | 10,1 | 8,8 | 8,8 |
| Total (24 procesos) | 92,9 | 84,8 | 66,1 |

En el total entran también `archivos` (34-36 MB, cambia de una pasada a otra), `comprobador` y `compositor`, que no
se tocaron. Después, el workbench no pasa de 256 KB de montón en toda la prueba (antes, 16 MB al final) y el guardián
de 4 MB (antes, 8).

Tiempo de pintar Ajustes entero (`i.dibujar`, 360 × 320; medido aparte, sin dejarlo en el código):

| | 1.ª vez | Siguientes |
|-|---------|-----------|
| `fontdue` | 9,8 ms | 3,2-5,2 ms |
| Texto nuevo | 11,4 y 13,4 ms (dos pasadas) | 3,3-5,0 ms |

La primera vez cuesta 1,5-3,5 ms más: rasteriza las letras que `fontdue` ya tenía en líneas (a cambio, cargar las dos
fuentes ya no cuesta 2 ms al arrancar). Con la caché llena, igual. Una sola pasada de cada uno con el PC cargado: son
cifras orientativas.

### Pruebas

- `make arranque-virt`: las cinco líneas BIEN (con todo; y también con 2 y 3 solos).
- `make prueba`: ver abajo.
- `cd kit && cargo test` (61) y `cd wayland && cargo test --lib` (49).
- La referencia, sin cambios: `make arranque-virt` falló una vez en "[workbench] no abre Sistema/DeCarla: -12" (el
  workbench se quedó tras abrir la ventana de Sistema). La siguiente pasada, con solo los contadores, fue bien.

### Lo que queda

- El fondo: 8,8 MB. Su montón llega a 4 MB en uso al decodificar y escalar la imagen (1,7 + 2,3 + 1,9 MB) y el montón
  nunca devuelve memoria al kernel: se queda con 6,8 MB reservados. Habría que decodificar a una memoria propia que se
  suelte, o que el montón suelte sus trozos de arriba cuando quedan vacíos.
- El montón no devuelve nada al kernel (en ningún programa).
- Rasterizar es más lento por letra que en `fontdue` (en el PC, 3,6 µs frente a 1,2 µs); con coma flotante por
  software se nota en la primera pintura. Se puede afinar (menos `floorf`, enteros en el acumulador).
- Cada `Contexto` tiene su caché de glifos: el workbench rasteriza otra vez las letras en cada ventana nueva. Poca
  memoria, algo de tiempo.
- Las líneas del montón al pedir trozos (`con_nombre`) se quedan: son pocas (1 o 2 por programa) y dicen lo que hay.
  Si estorban, se quitan las llamadas de los ocho programas.
