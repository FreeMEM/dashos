# El sistema de archivos de DashOS (S5): plan y bitácora

Decisión del usuario (2026-11-07): partir de RedoxFS. Llevamos una rama propia, con nuestro formato. Lo que sirva
también a Redox (arreglos, pruebas, reparación) se les manda, siempre con permiso del usuario. El diseño está en
`docs/diseño/S5-archivos.md`.

## Dónde está

- `archivos/redoxfs/`: RedoxFS como subárbol de git (base: 5ba45a6, 0.9.1, MIT). Para traer sus cambios:
  `git subtree pull --prefix=archivos/redoxfs https://gitlab.redox-os.org/redox-os/redoxfs.git master --squash`.
  Cada cambio nuestro va en su propio commit. El commit dice si es "para enviar a Redox" o "solo DashOS".
- `usuario/src/bin/archivos.rs`: el servicio de archivos. Va con la opción `archivos` de `usuario` (RedoxFS y el
  montón), que la caja big-endian no compila.
- `usuario/src/monton.rs`: el montón de un programa (`dashos::monton!()`), sobre trozos de 4 MB del kernel.
- `tools/disco_archivos.py`: el disco de prueba del NVMe. El primer MB es de la prueba de U6f y el resto es un RedoxFS
  hecho con `redoxfs-ar`.
- Prueba: `make -C kernel arranque-virt` (bajo el cerrojo del emulador).

## Lo que se vio de RedoxFS (2026-11-07)

- Unas 9.000 líneas; 575 commits desde 2016, 396 de Jeremy Soller; 47 pruebas (`cargo test`).
- Formato versión 8; rechaza las versiones anteriores sin convertirlas. Sin herramienta de reparación.
- Números en el disco en little-endian fijo (`endian_num::Le`): un PC lee el mismo disco.
- Sumas de cada bloque (seahash), copia al escribir, compresión LZ4. Sin instantáneas.
- Cifrado: AES-128-XTS del disco entero, con la clave protegida por la contraseña (argon2). No hay carpeta cifrada
  por usuario ni Adiantum.
- `FileSystem::open` busca la cabecera en los 65536 primeros bloques si no se le dice dónde: pasar `Some(0)`.
- No vaciaba nunca la caché del disco, y con el inicio dado solo miraba el hueco 0 del anillo de cabeceras: los dos,
  arreglados en FS2.

## Tareas

| Tarea | Qué | Estado |
|---|---|---|
| FS0 | RedoxFS en el repo; compila sin std (arreglo para Redox) | hecha |
| FS1 | Servicio que lee un RedoxFS por el canal "disco", en QEMU virt | hecha |
| FS2 | Pruebas en Linux de cortar la luz: cortar las escrituras en cada punto y comprobar que el disco abre con lo viejo o lo nuevo | hecha (`archivos/redoxfs/tests/power_cut.rs`) |
| FS3 | API de ficheros por canal para los clientes (abrir, leer, listar, datos de un fichero), en `docs/referencia/servicios.md` | hecha |
| FS4 | Escribir desde DashOS (crear, escribir, borrar, renombrar), con las pruebas de FS2 pasando | hecha |
| FS5 | Formato propio: marca "DashFS", versión propia y conversión desde la anterior | hecha (DashFS 1) |
| FS6 | Nombres sin distinguir mayúsculas (guardando cómo se escribieron) | hecha (DashFS 2, marca por carpeta) |
| FS7 | Atributos del Amiga: comentario y bits de protección | hecha (DashFS 3) |
| FS8 | Herramienta de reparación (comprobar y arreglar) | hecha (`redoxfs-fsck`, orden 16 del servicio) |
| FS9 | Instantáneas | hecha (DashFS 4, la época en la dirección de cada bloque) |
| FS10 | Carpeta cifrada por usuario con Adiantum | decidida: A2 como ZFS |
| FS11 | Cuidar la SD: escrituras juntas, TRIM, alineación a 4 MB | |

Con FS1 a FS4 ya puede empezar la caja Amiga (H4) y su `dos.library`.

## FS9, instantáneas

Decisión del usuario (2026-11-08): A, la época en la dirección de cada bloque.

Una instantánea es el volumen tal como estaba en un momento, para leerlo o para volver a él (S5: `SYS:`,
`Paquetes:`, las de cada hora de `Usuarios:`). Con copia al escribir, guardar la raíz vieja es gratis. Lo difícil
es soltar bloques: al cambiar un fichero, su bloque viejo solo se puede soltar si ninguna instantánea lo usa.
RedoxFS hoy lo suelta siempre. Hay que saber, de cada bloque, si alguna instantánea lo ve.

- **A (recomendada): la época en la dirección de cada bloque.** El número de bloque tiene 56 bits; 36 bastan
  (256 TB). Los otros 20 guardan la época: cuántas instantáneas había cuando se escribió el bloque. Al soltar un
  bloque: si es de la época de ahora, se suelta; si es de antes, alguna instantánea lo ve y va a su lista de
  pendientes. Crear una instantánea es inmediato (sube la época y guarda la raíz). Borrarla solo recorre lo que
  cambió después de ella. Y deja saber qué cambió entre dos instantáneas, para copias de seguridad por partes.
  Los discos de antes tienen ceros ahí, época 0: se abren tal cual. Límite: un millón de instantáneas en la vida
  del volumen (una por hora, 119 años). Es lo que hace ZFS, con la época en vez de su número de transacción.
- **B: un mapa de bloques por instantánea.** Al crearla, se recorre el disco entero y se guarda qué bloques usa,
  como hace el comprobador de FS8. No toca el formato de los punteros, pero crear y borrar cuesta leer todos los
  metadatos: segundos o minutos en una SD llena, cada hora.
- **C: punteros más grandes**, de 24 bytes con la época, como ZFS. Cambia todo el formato y hay que reescribir los
  discos, para lo mismo que A.

Las instantáneas son del volumen entero: en RedoxFS los cajones apuntan a los nodos por número, a través del
árbol, así que congelar un cajón suelto pide congelar el árbol. Una instantánea de un cajón se ve como la del
volumen, mirando solo ese cajón. Por eso `Usuarios:` conviene que sea un volumen aparte.

## FS10, carpeta cifrada por usuario

Decisión del usuario (2026-11-08): A2 "como ZFS".

- Varios volúmenes en un mismo disco (el común y uno por usuario), como los datasets de ZFS o los volúmenes de
  APFS. Comparten el espacio libre; un máximo por usuario es opcional. Sin fichero en medio: se escribe una vez y
  lo borrado vuelve al momento.
- Cada volumen tiene su llave, guardada en su cabecera y envuelta por la contraseña (Argon2, como los 64 huecos
  de RedoxFS): cambiar la contraseña no recifra nada, y cabe una llave de recuperación.
- Se cifra el contenido, los nombres, los permisos y los atributos. La estructura queda a la vista (cuántos
  ficheros, tamaños, nombres de volúmenes e instantáneas), como en ZFS: así FS8 comprueba y repara sin
  contraseñas y se puede copiar una carpeta cifrada sin abrirla.
- Adiantum en vez de AES (la Pi 4 no tiene instrucciones de AES). Adiantum no firma: cada bloque cifrado lleva su
  firma para notar si alguien lo ha cambiado.
- Pendiente de decidir: si hay llave de recuperación por defecto (un código en papel al crear el usuario). Sin
  contraseña ni llave de recuperación, los ficheros se pierden.

Pasos:

| Paso | Qué | Estado |
|------|-----|--------|
| FS10a | Adiantum (XChaCha12, AES-256, NH, Poly1305) desde la especificación, con los vectores de Google | hecho |
| FS10b | Varios volúmenes en un disco: cada uno con su árbol, su lista por soltar, sus instantáneas y sus listas de muertos; el espacio libre es común. El volumen de hoy es el primero | hecho (DashFS 5, `redoxfs-volume`) |
| FS10c | Cifrar un volumen como ZFS: llave propia envuelta por la contraseña, contenido y nombres con Adiantum (el tweak es la dirección del bloque), firma de cada bloque en el hash de su puntero | hecho (`seal.rs`, `redoxfs-volume create-encrypted`) |
| FS10d | Comprobar y reparar sin llaves: la estructura sí, el contenido cifrado no | hecho (cada volumen en su turno; dos copias de la lista y de las llaves) |
| FS10e | DashOS: abrir el volumen del usuario al entrar, y la llave de recuperación | órdenes 21 a 26 del servicio; crear cifrados desde DashOS espera azar de calidad |

Diseño de FS10c (el cifrado de un volumen):

- La llave del volumen (32 bytes al azar) cifra con Adiantum; de ella sale también la llave de las firmas
  (SipHash-2-4, 128 bits). En la lista de volúmenes, el volumen apunta a un bloque de llaves con hasta 8 huecos:
  cada uno guarda su sal, los parámetros de Argon2 y la llave envuelta (cifrada con Adiantum con la llave que sale
  de la contraseña, y 16 bytes a cero detrás para saber si la contraseña es buena). Cambiar la contraseña reescribe
  solo su hueco; la llave de recuperación es otro hueco.
- Qué se cifra (el tweak es la dirección del bloque): los datos de los ficheros, los bloques de las carpetas (los
  nombres y el índice, que lleva hashes de los nombres), los atributos (el comentario) y, de cada nodo, el dueño,
  los tiempos, el tamaño, los enlaces, los permisos y los bits del Amiga. Queda a la vista: los punteros (qué
  bloques usa cada nodo), si es fichero o carpeta, y el árbol de nodos.
- La firma de cada bloque cifrado va en el hash de su puntero: SipHash con la llave del volumen sobre el
  contenido sin cifrar. No deja comprobar adivinanzas del contenido, como haría el hash de siempre, y un bloque
  cambiado o movido a otra dirección no pasa (Adiantum lo deja irreconocible).
- Sin la llave: se recorre la estructura y se sabe qué bloques usa el volumen (FS8 comprueba el espacio); no se
  comprueban enlaces, nombres ni contenido. Con la llave, todo.
- Límite, como en ZFS: la estructura a la vista no va firmada con llave. Quien pueda escribir en el disco puede
  cambiar punteros, por ejemplo devolver un bloque a una versión vieja en la misma dirección. No puede leer ni
  fabricar contenido.

Descartadas: A (un fichero cifrado por usuario, como FileVault 1 y systemd-homed: escribe dos veces y recuperar
espacio cuesta) y B (cifrar fichero a fichero, como fscrypt: más complicado para lo mismo que A2 como ZFS).

## Para enviar a Redox (con permiso)

- FS0: los `use` de `alloc` que faltaban para compilar sin std (629bc2e).
- FS2: `Disk::flush` antes y después de la cabecera, y `tests/power_cut.rs` (4fddf69).
- FS2: `open` con el inicio dado busca en todo el anillo de cabeceras (642b3fc).
- FS4: `rename_node` no deja mover un cajón dentro de sí mismo (e8b2f76).
- FS8: borrar o acortar un fichero suelta también su último trozo a medias (a5652ad). Antes, cada fichero borrado
  cuyo tamaño no era múltiplo de 128 KB dejaba hasta 128 KB perdidos.
- FS9: escribir más allá del final tras acortar un fichero deja el hueco a ceros (175734b). Antes se leían los
  datos que se habían quitado.
- FS8: `DirList` con la disposición fijada (`repr(C, packed)`, cuentas al final en little-endian), la misma que ya
  dejaba el compilador (694bc7c).
- FS8: el comprobador y la reparación (`check.rs`, `repair.rs`, `redoxfs-fsck`), sin lo que es solo de DashFS
  (mayúsculas, atributos), y que abrir un disco con la lista de libres rota dé error en vez de abortar.

## Bitácora

### 2026-11-07

- FS0: subárbol en `archivos/redoxfs`; compila para `aarch64-unknown-none-softfloat` sin std tras añadir tres `use`.
  `cargo test` de RedoxFS en Linux: 47 pasan.
- FS1: montón de usuario, servicio `archivos` (solo lectura: el disco devuelve `EROFS` al escribir), disco de 8 MB.
  En QEMU virt abre el RedoxFS, lista la raíz, lee `LEEME.txt`, comprueba los 313.890 bytes de `C/grande.txt` y
  da `ENOENT` en lo que no existe. Con los 64 KB de pila de siempre se caía al listar: lleva 1 MB (`usuario/build.rs`).
  `make -C kernel arranque-virt` y `make -C kernel prueba` (raspi4b): BIEN.
- FS2: `tests/power_cut.rs` graba las escrituras de 7 transacciones (69, algunas de 128 KB; datos que LZ4 no
  encoge) y rehace el disco cortado tras cada una, a medias por sectores y desordenado entre dos vaciados. Encontró
  dos fallos de RedoxFS: sin vaciar la caché, la cabecera podía llegar antes que sus bloques (69 cortes ilegibles), y
  con `open(.., Some(0), ..)` un corte al reescribir el hueco 0 del anillo dejaba el disco sin abrir. Arreglados; se
  comprobó que la prueba falla sin cada arreglo. El servicio NVMe tiene la orden 5, vaciar (NVMe Flush).

### 2026-11-08

- FS3 y FS4 (a779b38): el protocolo "archivos" (`docs/referencia/servicios.md`) y escribir desde DashOS; el cliente
  `ficheros` lo prueba todo en QEMU virt.
- FS5: el formato se llama DashFS (firma `DashFS\0\0`, versión 1). La versión 1 tiene la misma disposición que RedoxFS
  8: un disco RedoxFS 8 se abre tal cual y la siguiente escritura pone la cabecera nueva (`Header::update`). Prueba
  `redoxfs_header_becomes_dashfs_on_write` (cabeceras pasadas a RedoxFS 8, abrir, escribir, volver a abrir; firmas
  o versiones desconocidas, rechazadas). Solo DashOS: Redox ya no abre nuestros discos. `cargo test`: 39 + 5 + 9
  bien; `make -C kernel arranque-virt` y `prueba`: BIEN; las cabeceras del disco de prueba dicen DashFS.
  Cuando cambie la disposición (FS6, FS7), DashFS 2 y una conversión de verdad.
- FS6, decidido por el usuario: una marca por carpeta (`NodeFlags::CASEFOLD`), como el casefold de ext4, en vez de
  todo el sistema. La raíz la lleva (`redoxfs-mkfs --exact-names` la quita) y cada carpeta nueva hereda la de su
  madre; `Transaction::set_casefold` la cambia solo en una carpeta vacía (como ext4: con nombres dentro, dos podrían
  volverse uno). El nombre se guarda como se escribió y se busca por su clave: NFD(casefold(NFD(c))) por carácter y
  orden canónico de las marcas. Así "Árbol", "ÁRBOL" y "A" + tilde aparte son el mismo nombre, y "Straße" y
  "STRASSE" también. Plegado por defecto, no el turco. Las tablas (`src/casefold_data.rs`, Unicode 16) salen de
  `tools/gen_casefold.py` y quedan fijas: si cambiaran, cambiarían las claves de nombres ya escritos, y eso pide
  otra versión de DashFS. Una carpeta marcada pone el hash del H-tree sobre la clave.
  Versión 2 del formato: la disposición no cambia; un disco DashFS 1 no tiene carpetas marcadas, así que se abre
  tal cual y sigue distinguiendo mayúsculas en todas sus carpetas. Un lector de DashFS 1 rechaza la versión 2.
  Renombrar cambiando solo cómo se escribe ("readme" a "ReadMe") cambia el nombre guardado; con `no_replace` y el
  mismo texto da EEXIST, como `renameat2` de Linux; dos enlaces duros del mismo fichero siguen siendo dos entradas.
  Pruebas: `src/casefold.rs` (casos, que la clave de una clave es ella misma en todos los caracteres, y 3.000
  cadenas mezcladas iguales a las de Python, `tests/casefold_vectors.txt`) y `src/tests_casefold.rs`: raíz y
  herencia, nombres con y sin tilde, carpetas que distinguen, cambiar la marca, renombrar, enlaces duros, 5.000
  nombres largos (H-tree de dos niveles) buscados y borrados con otras grafías, 6.000 operaciones al azar contra un
  modelo con tres semillas reabriendo el disco, y DashFS 1 a 2. La prueba al azar encontró un fallo: renombrar "ñ" a
  "n" + tilde aparte, con los dos argumentos iguales, no cambiaba el nombre guardado; ahora compara con el guardado.
  `cargo test`: 55 + 5 + 9 bien.
- FS7, DashFS 3: cada nodo lleva, en lo que era relleno, los bits del Amiga (`AmigaBits`: archivado, puro, guion,
  residente y "no borrar" para dueño, grupo y otros; leer, escribir y ejecutar son los de Unix) y un puntero a un
  bloque de atributos (`attributes.rs`: clave y valor; el comentario es "amiga.comment", y servirá para los atributos
  de la caja Linux). Ceros, que es lo que tienen los discos de antes, son lo que AmigaOS da a un fichero nuevo y
  ningún atributo: DashFS 1 y 2 se abren tal cual. El bloque se escribe de nuevo en cada cambio (copia al escribir),
  se suelta al quedar vacío y al borrar el fichero. Escribir o cambiar el tamaño quita el bit de archivado.
  `clone` copia los bits y los atributos, y ahora también si cada carpeta distingue mayúsculas (faltaba en FS6:
  clonar un disco con "a" y "A" en una carpeta que distingue fallaba en una que no).
  Protocolo: órdenes 13 comentario, 14 comentar y 15 proteger (bits y permisos); buscar y datos dan los bits en
  c >> 32; cada entrada de listar lleva bits y comentario; -12 protegido al escribir sin permiso de escritura o
  borrar con "no borrar". `ver` enseña bits y comentario.
  Pruebas: `src/tests_amiga.rs` (valores de partida, comentarios de 79 letras con eñes, atributos que no caben,
  los bits y el de archivado, 600 ficheros con comentario creados y borrados y 600 comentarios reescritos sin perder
  un bloque, borrado mientras está abierto, DashFS 2 a 3, clonar); se comprobó que la de bloques perdidos falla si
  no se suelta el bloque al borrar. `tests/power_cut.rs` con dos pasos más (comentarios, bits y escritura que quita
  el archivado): cortar la luz en cada escritura deja lo viejo o lo nuevo. En QEMU, `ficheros` busca sin mirar
  mayúsculas, pone y lee el comentario, lo ve en la lista, y comprueba los rechazos por protección; `ver` lo ve en
  Linux. `cargo test`: 66 + 5 + 9 bien.
- FS8: `src/check.rs` comprueba un disco sin escribir nada: la lista de libres, la de nodos por soltar, el árbol de
  nodos y sus marcas, cada nodo (tipo, marcas, bits, atributos, cuenta de bloques), cada cajón (su índice en orden,
  nombres buenos y sin repetir, cada entrada en su sitio del índice y hacia un nodo que existe) y, si se pide, cada
  trozo de datos con su suma. Cuenta quién usa cada bloque: usados dos veces, usados pero libres, perdidos, fuera
  del disco; enlaces contra entradas; nodos a los que no lleva ningún cajón. `src/repair.rs` arregla lo que se
  puede en una sola transacción: rehace el árbol sin los nodos ilegibles (y una raíz nueva si se perdió), rehace
  cada cajón con las entradas buenas, da a cada dueño su copia de un bloque compartido, pone los nodos sin cajón en
  `/Perdidos` por su número (no borra nada: la cuenta de enlaces puede ser lo roto), corrige cuentas y escribe la
  lista de libres de nuevo. Mientras repara solo escribe en bloques que el disco no usa, así que cortar la luz deja
  el disco como estaba o reparado. Los datos ilegibles se quedan como están, y se dicen.
  Encontró un fallo de RedoxFS: al borrar o acortar, el último trozo de un fichero, si no estaba lleno, nunca se
  soltaba (arreglado en su propio commit, para enviar a Redox; la prueba falla sin él: 96 bloques perdidos).
  También: `FileSystem::open_to_check` abre un disco con la lista de libres rota; abrirlo normal da error en vez de
  abortar.
  Órdenes: `redoxfs-fsck [--data] [--repair] DISCO` en Linux; `ver DISCO DESDE --comprobar`, que ahora usa
  `tools/disco_archivos.py` tras la prueba en QEMU; la orden 16 "comprobar" del servicio, que `ficheros` pide al
  final (el disco que escribió DashOS, sin un problema).
  Pruebas (`src/tests_check.rs`): discos sanos sin avisos, con cada clase de daño hecha a mano (bloques ilegibles
  de nodo, cajón, árbol, atributos, datos, listas; enlaces y cuentas mal; bloques perdidos, usados y libres,
  compartidos; nodos sin cajón, entradas a nada, nombres repetidos o imposibles, nombres fuera de sitio en el índice,
  marcas del árbol) encontrada y reparada, con lo que no estaba roto intacto; 300 rondas de daños al azar en
  bloques en uso, siempre reparadas dejando solo datos ilegibles; un corte de luz en cada escritura de una
  reparación; y ficheros de todos los tamaños escritos, recortados y borrados. La prueba de cortar la luz
  (`tests/power_cut.rs`) y la de operaciones al azar de FS6 comprueban ahora el disco en cada paso.
  Visto al terminar: `DirList` guardaba su cuenta y su largo en el orden de bytes de la CPU, y el sitio de cada
  campo lo elegía el compilador (`repr(Rust)`): los puso al final del bloque. Ahora es `repr(C, packed)` con ese
  mismo orden y `Le<u16>`: los discos de siempre se leen igual (comprobado con un disco hecho antes del cambio), y
  un DashOS big-endian u otro compilador también. Prueba `layout_on_disk`.
- FS9, DashFS 4 (decidido por el usuario: opción A). La dirección de cada bloque guarda, en sus 20 bits altos, la
  época en que se escribió: cuántas instantáneas se habían tomado. El número de bloque se queda en 36 bits (256 TB).
  La cabecera guarda la época, la de la instantánea más nueva, la lista de instantáneas y la lista de muertos del
  volumen (`src/snapshot.rs`). Al soltar un bloque que una instantánea ve, va a la lista de muertos en vez de
  liberarse. Tomar una instantánea es inmediato: guarda la raíz del árbol y se lleva la lista de muertos. Borrarla
  libera, de la lista de muertos de la siguiente, lo escrito después de la anterior. Volver a la más nueva libera lo
  escrito después de ella, bajando por el árbol solo donde hay bloques nuevos. La lista por soltar, la de libres, la
  de muertos y la de instantáneas no son del árbol: se liberan siempre. Los discos de antes tienen época 0 y se abren
  tal cual. `FileSystem::snapshot_tx` lee una instantánea; `redoxfs-snapshot DISCO [list | take | delete | rollback]`
  en Linux; órdenes 17 a 20 del servicio. El comprobador recorre también los árboles de las instantáneas (sus
  bloques se comparten, no están "usados dos veces") y mira que cada bloque de una lista de muertos lo vea alguna.
  Encontró otro fallo de RedoxFS: tras acortar un fichero, escribir más allá del final dejaba ver en el hueco los
  datos quitados (arreglado en su propio commit, para enviar a Redox).
  Pruebas (`src/tests_snapshot.rs`): 4.500 operaciones al azar contra un modelo, con más de 350 instantáneas tomadas,
  casi todas borradas y 180 vueltas atrás; tras cada una, el volumen y cada instantánea iguales a su copia del modelo
  y el comprobador sin avisos; al final, borrándolo todo, el espacio libre vuelve a ser el del principio. Nombres y
  errores, 100 instantáneas en varios bloques, una tomada a mitad de una transacción, DashFS 3 a 4, y reparar un
  volumen con instantáneas sin tocarlas. `tests/power_cut.rs` con cinco pasos de instantáneas. Se comprobó que la
  prueba falla si al soltar no se miran las instantáneas. En QEMU, `ficheros` toma una, crea un fichero, vuelve a
  ella (el fichero ya no está, las asas de antes no valen) y la descarta. Esa prueba encontró que al volver se
  perdían los ficheros borrados mientras estaban abiertos (quedaban sin cajón): cada instantánea guarda ahora su
  copia de la lista por soltar, y al volver se sueltan, porque ya nadie los tiene abiertos.
  Límites: solo se vuelve a la más nueva (como ZFS sin `-r`). La reparación no arregla una instantánea dañada
  (la dice), y al rehacer un cajón de un volumen con instantáneas sus bloques viejos no van a la lista de muertos:
  ese espacio no vuelve al borrar la instantánea (el comprobador lo dará como perdido y la reparación lo recupera).
- FS10a: Adiantum escrito desde la especificación (`src/adiantum.rs`), con `chacha20` y `poly1305` de RustCrypto
  para las piezas. Pasan 33 vectores de Adiantum (16 a 4096 bytes, tweak de 0, 17 y 32 bytes) y los de NH y
  NHPoly1305, sacados de github.com/google/adiantum (MIT) a `src/vectores/`.
- FS10b: DashFS 5, varios volúmenes en un disco (`src/volume.rs`). La cabecera guarda los campos del volumen
  principal y apunta a una lista de los demás (cadena de bloques, 21 por bloque). Una transacción entra en un
  volumen (`enter_volume`, `FileSystem::tx_in`) poniendo sus campos en su copia de la cabecera, así que el resto del
  código lo ve como el único; al escribir la cabecera, los del volumen van a la lista y los del principal a la
  cabecera. La época es del disco y `latest` de cada volumen: soltar un bloque mira solo las instantáneas de su
  volumen. Los abiertos se cuentan por volumen y nodo. Borrar un volumen borra sus instantáneas y suelta su árbol
  entero; con algo abierto da EBUSY. El comprobador recorre todos los volúmenes (lo de otro volumen sale como
  `InVolume`); la reparación arregla el principal y deja los demás en uso, y no repara si no puede leer la lista de
  volúmenes o entrar en uno (sus bloques parecerían libres). Pruebas: un modelo con 3 semillas × 1200 pasos
  (volúmenes que se crean y se borran, instantáneas en cada uno, dos volúmenes en una transacción, reabrir); al
  final, borrado todo, vuelve todo el espacio. Cortes de luz: 4 pasos más con volúmenes. Si el comprobador no
  recorre los otros volúmenes, las pruebas fallan (bloques perdidos).
  Pendiente: el máximo por volumen se guarda pero no se aplica; reparar dentro de otros volúmenes (FS10d); el
  montaje FUSE y el servicio de DashOS solo ven el principal (FS10e).
- FS10c: volúmenes cifrados (`src/seal.rs`). Cada tipo de bloque dice cómo se cifra (`BlockTrait::SEAL`): los
  datos, los bloques de las carpetas y los atributos, enteros; los nodos, solo lo que cuenta del fichero (permisos,
  dueño, tiempos, tamaño, enlaces, bits del Amiga), y el tipo queda a la vista para recorrer la estructura sin la
  llave. Las hojas del árbol se leen como bloques sin tipo: el árbol las lee y escribe como nodos
  (`read_block_as`, `sync_block_as`), que fue el primer fallo de las pruebas. La firma de un bloque cifrado
  (SipHash-2-4 con llave, sobre la dirección y el contenido) va en el hash de su puntero; el índice de las carpetas
  hashea los nombres con otra llave del volumen (`Names::keyed`). La llave del volumen va en un bloque de llaves
  con 8 huecos (contraseña o código de recuperación), envuelta con Argon2id + Adiantum y 16 bytes a cero para
  notar una contraseña mala. Sin la llave: leer da EACCES, escribir también, y el comprobador recorre solo la
  estructura (qué bloques usa). Se pueden tomar instantáneas y borrar el volumen sin la llave, como en ZFS.
  Pruebas: el nombre, la carpeta, el comentario y los datos no están en el disco (y sí en un volumen sin cifrar);
  contraseña mala; recuperación; quitar la última contraseña no se deja; un bit cambiado en un bloque da EIO y el
  comprobador lo ve (y sin la comprobación de la firma la prueba falla); instantáneas, volver y borrar sin llave
  devolviendo todo el espacio; el modelo de volúmenes con la mitad cifrados, cerrándolos al reabrir; cortes de luz
  con un volumen cifrado. Argon2id con 64 MiB y 3 pasadas tarda 0,1 s en este PC; en la Pi 4 no se ha medido.
  Límite: el tamaño comprimido de cada trozo de un fichero se ve, como en ZFS.
- FS10d: el comprobador da el informe del volumen en que está la transacción, y lo de los demás como `InVolume`
  (el principal sin nombre). La reparación entra en cada volumen por turno (el principal, los no cifrados y los
  cifrados abiertos) y lo arregla como si fuera el único; uno cerrado solo se comprueba. Las copias de nodos de la
  reparación van por bloques con tipo, así que un nodo cifrado se copia descifrado y vuelto a cifrar con su nueva
  dirección. Lo crítico va dos veces, como las copias "ditto" de ZFS: la lista de volúmenes (dos cadenas, la
  segunda en `Header::volumes_copy`) y el bloque de llaves de cada volumen cifrado. Con una copia dañada no se
  pierde nada y la reparación la rehace; con las dos de la lista, la reparación se niega; con las dos de las llaves,
  ese volumen ya no se puede abrir y el comprobador lo dice (`Keys`). Un fallo de las pruebas: la reparación
  rehacía las llaves en cada ronda porque la comprobación leía la lista del disco y no la de la transacción; ahora
  el comprobador escribe antes los cambios pendientes de la lista. Pruebas: daños en un volumen normal y en uno
  cifrado (con mutación: si la reparación no entra en los otros volúmenes, falla); 150 rondas de daños al azar en
  un disco con tres volúmenes, uno cifrado con instantánea (lo que queda es solo datos ilegibles, o una instantánea
  dañada, que FS9 dice y no repara); cada copia dañada por separado y las dos a la vez.
- FS10d, después: una lista de instantáneas ilegible ya no se queda así: la reparación olvida las instantáneas de ese
  volumen y sus listas de muertos (estaban perdidas igualmente), y lo que solo ellas veían vuelve al espacio libre.
  Antes, cada escritura en ese volumen dejaba bloques perdidos (lo encontró la prueba de daños al azar).
- FS10e: el servicio de archivos con volúmenes. Cada asa lleva su volumen; la orden 21 da la raíz de uno por su nombre
  y lo demás se hace desde ahí; renombrar de un volumen a otro no se deja. Órdenes 22 (lista, con las marcas cifrado
  y abierto), 23 (hacer, sin cifrar), 24 (quitar), 25 (abrir con contraseña) y 26 (cerrar); estado -13 "en uso". Las
  instantáneas (17 a 20) van al volumen de la asa, y volver suelta solo las asas de ese volumen. Prueba en QEMU
  (`ficheros`, "FS10 volúmenes"): el disco trae "Ana" cifrado (hecho en Linux con `redoxfs-volume create-encrypted
  Ana --test-cost`, Argon2 barato para que QEMU no tarde); cerrado no se lista, con otra contraseña no se abre, con la
  suya sí; se escribe dentro; con algo abierto no se cierra; cerrado no se busca dentro; se reabre y se lee; un
  volumen sin cifrar se hace, no se quita con algo abierto, y se quita; el disco queda sin problemas. En Linux,
  `disco_archivos.py --comprobar` mira que "secreto.txt" y su contenido no están en claro en el disco.
  Pendiente: crear volúmenes cifrados desde DashOS pide azar de calidad (el kernel solo mezcla el reloj: RNG200 en la
  Pi 4, virtio-rng en QEMU); abrir el volumen del usuario al entrar va con los usuarios (S6); el código de
  recuperación en papel, sin decidir; Argon2 con 64 MiB deja el montón del servicio en ese tamaño (no devuelve
  memoria).
- FS10c, fallo encontrado por la prueba de QEMU: el contenido de un fichero pequeño estaba en claro en el disco. RedoxFS
  guarda los ficheros pequeños dentro del nodo (`INLINE_DATA`), en la zona de los punteros, que no se cifraba. Ahora,
  en un nodo con esa marca, esa zona también va cifrada (Adiantum, otro tweak); la marca queda a la vista porque dice
  que no hay bloques que recorrer. Las pruebas en el PC usaban ficheros grandes y no lo vieron: `nothing_in_the_clear`
  lleva ahora uno pequeño (falla sin el arreglo) y la de ida y vuelta del nodo, un fichero con datos en línea.
  En la misma prueba de QEMU, listar "Ana" cerrado y vacío da cero entradas, no "protegido": una carpeta vacía no
  tiene bloques que descifrar. Es lo esperado con la estructura a la vista; la prueba lo comprueba así, y "protegido"
  al listar con algo dentro.

