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
| FS3 | API de ficheros por canal para los clientes (abrir, leer, listar, datos de un fichero), en `docs/referencia/servicios.md` | |
| FS4 | Escribir desde DashOS (crear, escribir, borrar, renombrar), con las pruebas de FS2 pasando | |
| FS5 | Formato propio: marca "DashFS", versión propia y conversión desde la anterior | |
| FS6 | Nombres sin distinguir mayúsculas (guardando cómo se escribieron) | |
| FS7 | Atributos del Amiga: comentario y bits de protección | |
| FS8 | Herramienta de reparación (comprobar y arreglar) | |
| FS9 | Instantáneas | |
| FS10 | Carpeta cifrada por usuario con Adiantum | |
| FS11 | Cuidar la SD: escrituras juntas, TRIM, alineación a 4 MB | |

Con FS1 a FS4 ya puede empezar la caja Amiga (H4) y su `dos.library`.

## Para enviar a Redox (con permiso)

- FS0: los `use` de `alloc` que faltaban para compilar sin std (629bc2e).
- FS2: `Disk::flush` antes y después de la cabecera, y `tests/power_cut.rs` (4fddf69).
- FS2: `open` con el inicio dado busca en todo el anillo de cabeceras (642b3fc).

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
