# Bitácora: el servicio de archivos sin memoria (archivos-memoria)

Rama `archivos-memoria`, sacada de `revision` (5470528a). Papel: implementa. No se leyó código ajeno a DashOS (solo la
API pública del crate `argon2` 0.4, MIT/Apache-2.0, que ya se usaba).

## 2026-11-16

### El problema

Crear una cuenta con el Argon2 de S6b (64 MiB) tumbaba el servicio de archivos (`docs/bitacoras/cuentas.md`), y la
rama `cuentas` lo bajó a 16 MiB para esquivarlo. La causa no era de Argon2: en Rust, una reserva que no cabe y no se pide
con `try_reserve` es un pánico, y un pánico acaba el programa. En el servicio había tres caminos a eso:

1. **El montón de los programas** (`usuario/src/monton.rs`, común a todos). Un pedido de más de 16 MB (lo más que da
   el kernel en un objeto) iba al montón, que crecía a trozos de 4 MB hasta que el pedido cupiera o el grupo topara.
   Al topar devolvía nulo (pánico), y los trozos pedidos se quedaban para siempre: tras un Argon2 de 64 MB fallido, el
   servicio tenía el grupo lleno de montón vacío y caía con lo siguiente que pidiera. Además, si el kernel daba la
   llave de un trozo y no lo dejaba proyectar, la llave (y su memoria) se quedaban abiertas.
2. **DashFS**: el Argon2 de cada ranura (`hash_password_into`, que reserva con `vec!`), el coste de esa ranura leído
   del disco sin topes (un disco hostil podía pedir 256 GiB o cuatro mil millones de pasadas), listar un cajón (todas
   sus entradas a la vez en un `Vec`), y las listas de una transacción que suelta muchos bloques (recortar un fichero
   de miles de trozos, quitar un volumen): todo con `push`.
3. **El servicio**: la copia de la ventana del cliente al escribir (hasta 1 MB), la de un fichero al copiar a `RAM:`
   (hasta 4 MB), y las ventanas de los clientes, que se cobran a su grupo: 64 clientes con ventanas de 1 MB llenaban
   los 64 MB, y entonces cualquier `Box` de la caché tumbaba el servicio.

### Lo hecho

**Montón (biblioteca común, todos los programas):**
- Lo de más de 16 MB va en su propio objeto **disperso** (llamada 57) con todas sus páginas puestas (58) antes de
  proyectarlo, en ranuras de 1 GB de 496 a 511 GB. Si no caben todas, se cierra (las puestas vuelven) y el pedido da
  nulo. Al soltarlo vuelve al kernel, como lo grande (de 256 KB a 16 MB) desde kit-memoria.
- El montón crece **una vez** por pedido: el trozo que toca o, si el kernel no lo da, lo justo. Un trozo que no basta
  no se pide. Lo grande que no tuvo su objeto porque el kernel no dio la memoria va al montón solo si ya hay hueco.
- La llave de un trozo que no se pudo proyectar se cierra.
- `Uso` cuenta los pedidos que no cupieron (`fallos`) y las veces que se gastó el colchón.
- **Colchón** (opcional, `monton::colchon`): memoria apartada en el montón que se suelta cuando el kernel ya no da
  más, para que lo pequeño que no puede fallar (bloques de la caché, nodos, tablas de DashFS) quepa hasta acabar la
  orden. El programa mira `en_apuro` y, si se gastó, suelta lo que pueda y lo repone (`rearmar`). Solo lo usa el
  servicio de archivos (2 MB).

**DashFS:**
- `Cost::is_reasonable`: hasta 1 GiB, 16 pasadas, 16 carriles y memoria por pasadas hasta 1 GiB (unos tres segundos
  en una Pi 4; el de serie es 192 MiB de eso). Una ranura fuera de eso no se prueba (un disco hostil), y no se puede
  poner.
- El Argon2 pide su memoria con `try_reserve_exact` y usa `hash_password_into_with_memory`: ENOMEM si no cabe.
- Abrir un volumen: ENOMEM si una ranura no cupo y ninguna otra vale (puede que fuera la buena); EINVAL si había
  ranuras y ninguna se pudo probar por su coste; EACCES si no.
- La transacción: si una lista de liberaciones o reservas no puede crecer, la transacción queda marcada
  (`out_of_memory`), no hace esa liberación, las reservas siguientes dan ENOMEM y `sync` (y `commit`) da ENOMEM. Al
  soltarla se deshace como cualquier otra (`undo`): el disco y el asignador quedan como estaban.
- Listar un cajón y la lista de bloques de un volumen que se quita: con `try_reserve`, ENOMEM.

**Servicio de archivos:**
- Estado nuevo **-15, `SIN_MEMORIA`** (de ENOMEM). La caja Amiga lo da como `ERROR_NO_FREE_STORE`, las órdenes como
  "no hay memoria para hacerlo".
- Colchón de 2 MB; tras cada mensaje, si se gastó, suelta la caché de bloques limpios y los nodos recordados y lo
  repone.
- Las copias de la ventana (escribir, guardar) y de un fichero (copiar a `RAM:`), con `try_reserve`.
- Las ventanas grandes de los clientes suman como mucho 16 MB; pasado eso, la de 64 KB (el cliente usa la que se le
  da).
- `PERFIL` (orden 98) dice también el montón: en uso, máximo, reservado, fallos y colchones.
- **Argon2 de 64 MiB otra vez** (`Cost::NORMAL`, el de S6b) en `HACER_VOLUMEN` cifrado, y el tope del grupo de
  archivos de 64 a 128 MB (`inicio`). `HACER_VOLUMEN` acepta además la memoria en MiB en los bits 32 a 47 de `a`.

### Las cuentas del tope

Lo del servicio, como mucho: 24 MB de caché de páginas (se crean al arrancar), 16 de caché de bloques, 4 de nodos
recordados, 2 de colchón, el programa y su pila (unos 3), y las ventanas (16 las grandes y 64 KB las demás: hasta 20).
Unos 70 MB en el peor caso; en las pruebas, mucho menos (ver lo medido). El Argon2 de 64 MiB va encima mientras dura
y vuelve al kernel al acabar. 128 MB dejan sitio a las dos cosas. El tope no aparta memoria (S3): lo que se usa de
verdad sube solo al hacer o abrir un volumen.

### Pruebas

- Resultados: `cargo test` de `archivos/dashfs` (171 + 5 nuevas + las de antes), `ordenes` y `caja/dos`: bien.
  `make arranque-virt-2` y `make arranque-virt-linux`: bien. `make arranque-virt`: las líneas de esta rama, bien en
  todas las vueltas (`[ficheros] memoria`, `[hostil] montón`, crear a Berta); la prueba entera falló en pasos del
  teclado y el ratón de otros programas (el desplegable de Ajustes, el clic en la ventana de Dash), distintos en cada
  vuelta; la de referencia, sin mis cambios, falló también en Ajustes. `make prueba`: falló en pruebas de tiempo del
  kernel, distintas en cada vuelta (S2, C14), con la máquina a carga 32; el kernel no se tocó.
- PC, `archivos/dashfs/tests/memoria.rs` (nuevo): un asignador que niega los pedidos de N bytes o más mientras dura
  cada caso. Si algo que no puede fallar pidiera tanto, el programa abortaría (así salieron los dos caminos de la
  transacción). Casos: hacer y abrir un volumen con un Argon2 que no cabe (ENOMEM, nada a medias, sigue cerrado, con
  memoria va), costes fuera de los topes (EINVAL sin pedir la memoria), un cajón de 3.000 entradas, recortar un fichero
  de 6.000 trozos y quitar un volumen con 6.000 trozos (ENOMEM, el fichero o el volumen siguen enteros, comprobación
  limpia; con memoria se hace). Y en `seal.rs`, una ranura hostil (cuatro mil millones de pasadas) que no se prueba.
- QEMU, `ficheros` (el cliente de pruebas con el derecho de administrar), línea nueva `[ficheros] memoria: bien`:
  hace un volumen con el Argon2 de 64 MiB, prueba una contraseña mala y la buena, y lo quita; mira en `PERFIL` que el
  montón del servicio no se queda con los 64 MB; pide uno de 4 GiB (fuera de topes: -1) y uno de 320 MiB (no cabe:
  -15, y no queda nada); abre 100 ficheros, los escribe sin guardar y pide otra vez el de 320 MiB (-15); relee los
  100, los guarda; escribe, lee y borra un fichero de 150 trozos; lee `LEEME.txt` y comprueba el disco entero con los
  datos (limpio).
- QEMU, `hostil` (programa de un usuario, grupo de 16 MB): pide 17 MB y 2 GB con `try_reserve`; los dos dan error, el
  montón no crece y lo pequeño sigue cabiendo. Y el colchón: llena su grupo con objetos de memoria hasta que el kernel
  dice que no, pide cajas de 1 KB hasta que se gasta el colchón (caben todas), suelta y lo repone. La línea del guion
  cambia: `[hostil] montón: lo grande va aparte y vuelve al kernel al soltarlo, lo que no cabe da error y el colchón
  aguanta: bien`.

### Lo medido

En QEMU (`make arranque-virt`, con la máquina muy cargada por otras pruebas: carga media de 25 a 32):

- Un volumen con el Argon2 de 64 MiB: hacerlo, 663 y 787 ms (dos vueltas); probar una contraseña mala, 589 y 861 ms.
- Crear una cuenta (dos ranuras, la contraseña y el código de recuperación): 3.438 ms, antes 236 ms con 16 MiB (en
  una máquina menos cargada). `sesiones` espera mientras.
- El montón del servicio (de `PERFIL`): 5,8 MB en uso y 10,9 MB pedidos al kernel antes y después del Argon2 de
  64 MiB: no se queda con nada. Al final de la prueba, 6,5 MB en uso; 2 pedidos que no cupieron (los de 320 MiB), y el
  colchón sin gastar.
- Lo que gasta el servicio entero (el monitor del centro): 38,2 MB; en `revision` del 15 de noviembre, 36,6 MB. Los 2
  MB de más son el colchón.
- No se midió el rendimiento del disco: lo que cambia en la transacción es un `try_reserve(1)` por bloque reservado o
  soltado, que no pide memoria si ya hay sitio.

### Pendiente del usuario

- **El tope del grupo de archivos**: 128 MB (antes 64), para el Argon2 de 64 MiB de S6b. La otra salida era quedarse
  en 16 MiB.
- **Los topes del coste de Argon2** de una ranura: 1 GiB, 16 pasadas, 16 carriles y 1 GiB × pasada. Un disco con más
  no se abre en DashOS (ni en Linux con las herramientas de DashFS).
- **La memoria de Argon2 elegible** en `HACER_VOLUMEN` (bits 32 a 47 de `a`): para que `sesiones` la ajuste a la
  máquina (una Pi de 1 GB frente a una de 8). Hoy nadie la pide; la usa la prueba.
- **El colchón** del servicio: 2 MB, y el de las ventanas grandes, 16 MB entre todas.
- **Crear una cuenta tarda más**: dos Argon2 de 64 MiB (la contraseña y el código), en una Pi 4 un segundo según S6b,
  frente a un cuarto con 16 MiB. Abrir el volumen al entrar, medio segundo.

### Lo que queda

- Mientras dura un Argon2 de 64 MiB (medio segundo en una Pi 4, más en QEMU), el servicio no atiende a nadie más: es
  un hilo. Ya pasaba con el de 16.
- Listar un cajón carga todas sus entradas a la vez: uno enorme da -15 en vez de tumbar el servicio, pero no se puede
  listar. Habría que listarlo por partes desde el disco.
- La comprobación al arrancar y el asignador de DashFS en memoria crecen con el disco (nodos, fragmentación) y no
  pueden fallar: un disco muy grande y muy troceado podría no caber. No es una petición de un cliente; no lo he tocado.
- El colchón cubre lo pequeño de una orden; algo grande de DashFS que aún se pide con `push` (el registro de asignación
  al escribir la transacción) podría tumbar el servicio si el kernel no da nada y el colchón no basta.
- Los demás servicios no tienen colchón: siguen cayendo si se quedan sin memoria en algo que no puede fallar (y los
  relanza `inicio`), aunque ya no por un pedido grande que no cabe.

### Lo que no se comprobó

- En la Pi: el tiempo del Argon2 de 64 MiB allí, y con poca memoria libre en la máquina (no solo en el grupo).
- Que el servicio de archivos suelte sus cachés al gastar el colchón (`aliviar`): el colchón se probó en `hostil`,
  pero la prueba de `ficheros` no llega a llenar el grupo del servicio con lo pequeño (en `PERFIL`, 0 veces).
- El rendimiento del disco antes y después (`medir-fs`).
