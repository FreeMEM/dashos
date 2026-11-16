# Bitácora: dashfs-tamano (lo que le faltaba a DashFS para el instalador y la herramienta de discos)

Rama `dashfs-tamano`, sacada de `revision` en 41e5b81. Encargo: `docs/diseño/S32-instalador.md`, "Lo que le falta a
DashFS para esto": encoger moviendo datos, aplicar la cuota de los volúmenes, crecer montado, y las pruebas de cortes.

## 2026-11-15 y 16

### Lo hecho

- **Encoger moviendo datos** (`archivos/dashfs/src/resize.rs`, `FileSystem::shrink`, `shrink_min`, `min_size`). Sin
  montar. Primero recorre todos los árboles (el de cada volumen y los de sus instantáneas) y apunta por qué bloques
  se llega a algo del tramo que se quita. Luego, por trozos de hasta 16 MiB del tramo, de abajo arriba, cada trozo en
  una transacción: copia cada bloque a uno libre de antes del corte, cambia los punteros que llevan a él hasta la
  cabecera y suelta el viejo. La última transacción pone el tamaño nuevo y rehace el registro de asignación sin el
  tramo. Antes de esa, vuelve a recorrerlo todo: si algo queda en el tramo, no corta (`Damaged`).
  - **Instantáneas**: el bloque mudado conserva su época, así que quién lo ve no cambia. Lo que comparten el volumen
    y sus instantáneas se muda una vez, y todos los árboles que llevan a él cambian en la misma transacción. Las
    listas de muertos se reescriben con las direcciones nuevas.
  - **Cifrado**: en un disco cifrado entero (AES-XTS con el número de bloque) y en un volumen cifrado abierto
    (Adiantum y firma con la dirección), mudar es descifrar y volver a cifrar y a firmar. Un volumen cifrado
    **cerrado** con algo en el tramo no deja encoger (`Locked`, con su nombre): sin la llave no se puede. Sus listas
    (que no van cifradas) sí se mudan, y si nada suyo está en el tramo, se encoge.
  - **El asignador** (`allocator.rs`): un tope (lo que se suelta más allá no vuelve), `free_run` y `truncate` (un
    tramo entero de una vez) y `allocate_snug` (del hueco más pequeño que sirva, para que lo pequeño llene los huecos
    pequeños y los grandes queden para los registros de 128 KiB).
  - **Errores, no `assert`**: `ResizeError` (`BadSize`, `TooSmall` con el mínimo, `NoRoom`, `Locked`, `Busy`,
    `Damaged`, `BeyondDisk`, `Io`). Si falla, el asignador en memoria se rehace del disco.
- **Crecer en uso** (`FileSystem::grow`). Una transacción: los bloques nuevos al asignador, una entrada en el
  registro y el tamaño en la cabecera. Vale con guardado aplazado (montado): lo nuevo se usa enseguida y el tamaño
  llega al disco con el siguiente guardado. En el servicio `archivos`, la orden 40 (`CRECER`), con el derecho de
  administrar.
- **Encoger montado: no se hace**, a propósito. Habría que mudar bloques de ficheros abiertos por debajo de quien los
  usa. Con guardado aplazado, con algo abierto o con una foto sujeta, `shrink` da `Busy`.
- **La cuota de los volúmenes** (`volume.rs`). Cada volumen lleva la cuenta de sus bloques, en los 7 bytes que eran de
  relleno en su entrada (sin cambio de formato). Una transacción que lo hace crecer por encima de su tope falla con
  "lleno" y no se confirma; a los demás volúmenes no les pasa nada. `set_volume_quota` cambia el tope y
  `volume_space` lo lee; en el servicio, la orden 41 (`CUOTA`); en el PC, `dashfs-volume DISCO quota NOMBRE TAMAÑO`.
  - **Qué cuenta**: los bloques de su árbol, los que solo ven sus instantáneas y los de sus listas. Un bloque que
    comparten el volumen y una instantánea cuenta **una vez**. Es lo que se liberaría al borrar el volumen entero.
    No cuentan los dos bloques de llaves de un volumen cifrado ni la lista de volúmenes, que son del disco.
  - **Cuándo se mira**: al acabar la transacción, no al reservar cada bloque (con copia al escribir se reserva lo
    nuevo antes de soltar lo viejo). Así borrar, recortar y escribir encima valen siempre, también pasado el tope.
  - Un volumen de antes (cuenta 0) se cuenta la primera vez que cambia. El comprobador compara la cuenta con lo que
    cuenta él (`Problem::VolumeUsed`) y la reparación la corrige. El volumen principal no tiene tope.
- **Una cabecera que dice más de lo que tiene la partición**: no se abre para usarla (`EINVAL`); el comprobador lo
  dice (`Problem::BeyondDisk`); y `shrink` hasta el disco lo arregla si en lo que falta no había nada. Un fichero de
  imagen no cuenta como límite (crece al escribir): `Disk::size_is_limit`.
- **`dashfs-resize`** usa la biblioteca: `DISCO [TAMAÑO | min | max] [--abrir VOLUMEN]`. Comprueba antes y no toca
  un sistema de archivos con daños.

### Pruebas (`cargo test` en `archivos/dashfs`: 169 en la biblioteca, todas bien; `archivos/herramientas`, bien)

- `tests_resize.rs`: encoger a la mitad y al mínimo un disco con directorios grandes, ficheros de todos los tamaños,
  dos instantáneas, otro volumen con la suya (se compara todo, también lo de cada instantánea; luego se borra una y
  se vuelve a otra); crecer y volver a encoger; un volumen cifrado abierto y cerrado; un disco cifrado entero; crecer
  en uso con guardado aplazado.
- **Cortes de corriente**: se apuntan todas las escrituras de encoger (en trozos de 24 bloques: 610 apuntes) y de
  crecer, y se corta en cada una, con y sin perder al azar parte de lo escrito desde el último vaciado. Cada vez el
  disco abre, `check` con datos sale limpio, el tamaño es el de antes o el nuevo y está todo. Después de algunos
  cortes se repite la operación y acaba bien. Solo la última escritura (la cabecera) lo deja encogido.
- **Fallo de escritura** en cada punto de encoger y de crecer: da el error del disco, el tamaño no cambia, y lo que
  queda es válido.
- **Hostiles**: por debajo de lo usado (`TooSmall`, dice el mínimo); tamaño no múltiplo de bloque (al múltiplo de
  antes); más grande que ahora; con el disco casi lleno; sin hueco seguido (`NoRoom`, y queda como estaba); crecer
  más que el disco o más de lo que cabe en una dirección; cabecera mayor que la partición, con y sin datos más allá;
  con instantáneas; con un volumen cifrado cerrado; encoger montado.
- `tests_quota.rs`: lleno en un volumen y no en los demás; hasta el tope bloque a bloque; escribir encima y borrar
  con el volumen lleno; subir, bajar por debajo de lo usado y quitar el tope; instantáneas (lo borrado que ve una
  instantánea sigue contando); un volumen de antes; una cuenta estropeada, vista y reparada; encoger no cambia la
  cuenta. Hostil: tope al principal, a un volumen que no existe, desde dentro de otro volumen.
- Las pruebas de modelo que ya había (volúmenes e instantáneas al azar, con `check` a cada paso) pasan con la cuenta
  nueva: si la cuenta se desviara, `check` daría `VolumeUsed`.
- En DashOS (`ficheros`, dentro de "FS10 volúmenes", sin líneas nuevas): tope de 80 KB a un volumen, lleno en él y no
  en el principal, sin tope cabe; `CRECER` a todo el disco, y no a menos ni a más.

- `make prueba`, `make arranque-virt`, `arranque-virt-2` y `arranque-virt-linux`: bien. `arranque-virt` falló una
  vez por una línea que no es de esto ("[dash-ventana] pulsado: Pulsado 1 veces; líneas: 2") y pasó al repetirlo.

### Lo medido (PC, `cargo test --release --lib medida_de_tamano -- --ignored --nocapture`, tres veces)

Imagen de 1 GiB en un fichero (NVMe), 540 MiB en uso entre los 256 y los 796 MiB:

| Qué | Tiempo | Mudado |
|---|---|---|
| Encoger a 768 MiB | 0,09 s | 30 MiB, 3 transacciones |
| Encoger al mínimo (541 MiB) | 0,28 a 0,29 s | 240 MiB, 19 transacciones |
| Crecer en uso (de 640 MiB a 1 GiB) | 11 a 16 µs | — |
| La escritura de 64 KiB más lenta, antes y después de crecer | 440 a 500 µs y 100 a 150 µs | — |

Crecer cuesta menos que una escritura normal: no para nada. Ojo: es un fichero en un PC, con la caché del sistema;
en un disco de verdad dentro de DashOS, encoger irá a lo que dé el disco. No se ha medido ahí.

### Decisiones pendientes del usuario

1. **Qué cuenta para la cuota**: he elegido que cuente también lo que solo ven las instantáneas (como `quota` de
   ZFS, no `refquota`). Si no, con instantáneas se llena el disco sin pasar del tope. La pega: borrar un fichero que
   una instantánea guarda no deja sitio.
2. **Poner el tope por debajo de lo usado** se deja (el volumen no crece hasta que baje). ZFS lo rechaza.
3. **Tamaño no múltiplo de bloque**: se redondea hacia abajo, al crecer y al encoger, en vez de dar error.
4. **Una cabecera mayor que la partición no monta** en DashOS. Antes montaba y fallaba al escribir más allá.
5. Los números de las órdenes nuevas son 40 y 41 (no 36 y 37) por si otra rama usa los siguientes.

### Lo que queda

- El servicio de disco no dice el tamaño nuevo de una partición que cambia sin reiniciar: `CRECER` crece hasta lo
  que el servicio vio al abrir. Hace falta para crecer de verdad en caliente; es de la herramienta de discos.
- `INFO` (orden 12) da lo libre del disco, no lo que le queda a un volumen con tope. `CUOTA` sí lo dice.
- Un volumen cifrado cerrado con datos en el tramo: el instalador tendrá que pedir la contraseña.
- Encoger no desfragmenta lo que queda por debajo del corte: si lo libre está en trozos pequeños, el mínimo real es
  mayor que lo usado (`shrink_min` lo busca).
- `dashfs-resize` no recorta el fichero de imagen ni descarta (TRIM) el tramo quitado.
