# S20. RAD: y las imágenes de disco del Amiga (ADF y HDF)

Del 2026-11-09. Lo pidió el usuario ese día ("deberíamos tener discos RAD como en Amiga") y aceptó la propuesta: RAD:
con copia opcional a la tarjeta, y ADF y HDF montables con copia al escribir. Es diseño: no hay código.

## El problema

- **RAD:** no estaba en ningún diseño. `RAM:` (S4b) es un disco en memoria por sesión que se pierde al apagar. En el
  Amiga, RAD: (el `ramdrive.device`) aguanta un reinicio en caliente: se usaba para arrancar deprisa y para no perder
  el trabajo cuando un programa tumbaba la máquina.
- **ADF:** solo aparecía en la máquina Amiga entera (S18), como disquete del emulador.
- **HDF:** S5 prevé leer y escribir FFS, SFS y PFS3, pero no dice cómo se monta una imagen.

Casi todo el software de Amiga que la gente guarda está en ADF y HDF: juegos, WHDLoad, instalaciones de Workbench.
Sin montarlos, pasar las cosas de un usuario a DashOS cuesta mucho.

## RAD:

### Cómo es

- **Un disco DashFS en una zona de memoria reservada.** El tamaño se elige en las preferencias, como el MountList
  del Amiga (`DEVS:DOSDrivers/RAD`), y lo lee el arranque, que reserva la zona antes de dar memoria a nadie.
- **Un volumen por usuario**, con los volúmenes de DashFS 5 (FS10): el de cada usuario va cifrado con su llave, que se
  abre al entrar (S14: la pantalla de entrada está siempre). En memoria y tras un reinicio, el contenido sigue
  cifrado: otro usuario, o quien arranque otra cosa en la máquina, no lo lee.
- **Lo ven igual** los programas nativos y los de la caja Amiga, como `RAD:`.

### Qué aguanta

- **Que se caiga la caja Amiga, un programa o la sesión:** RAD: es de su propio servicio, no de la caja.
- **Un reinicio en caliente de DashOS:** el kernel se vuelve a cargar a sí mismo sin pasar por el firmware, como el
  Amiga saltaba a su vector de reinicio, y no toca la zona de RAD:. Al arrancar, el servicio comprueba el disco con
  el comprobador de DashFS (FS8 y FS10d, sus códigos en cada bloque): si está entero, lo monta; si no, lo dice y lo
  rehace vacío (o desde la copia de la tarjeta).
- **Un reinicio por el firmware:** en QEMU con EDK2 y en la Pi, el firmware puede tocar la memoria. No se promete.
  En la Pi hay que probar en la placa si alguna zona alta de la RAM sobrevive al reinicio del vigilante (PM_RSTC).
- **Quitar la corriente, con la copia a la tarjeta (opcional):** al apagar, y cada cierto tiempo si se pide, el
  volumen de cada usuario se copia a un fichero en su cajón (cifrado como el cajón). Al arrancar, si la memoria no
  sirve, se carga la copia. Es algo que el Amiga no tenía.

### Lo que no hace

- No sustituye a guardar: la copia a la tarjeta va cada cierto tiempo, y lo último puede perderse.
- No es memoria para programas: es un disco, con su tamaño fijo.

## ADF y HDF

### Cómo se usan

- **Se montan como volúmenes.** Doble clic en un ADF o un HDF y aparece como un disco más, con su nombre de volumen
  y un nombre de aparato propio (`IMG0:`, `IMG1:`...). Lo ven los programas nativos y los de la caja Amiga.
- **"Meter en DF0:"**: para los programas que buscan la disquetera, la caja Amiga asigna `DF0:` a la imagen y la
  cambia cuando se pide, como cambiar de disquete.
- **Copia al escribir.** Por defecto, lo que se escribe va a una capa de cambios al lado de la imagen (los bloques
  cambiados, por número). Al desmontar se elige: guardar los cambios en la imagen o tirarlos. El original no se toca
  hasta entonces. También se puede montar de solo lectura, o escribiendo directamente si se pide.
- **Las mismas imágenes** sirven a la máquina Amiga entera de S18.

### Qué se lee

- **ADF:** disquetes de 880 KB (80 cilindros, 2 caras, 11 sectores de 512 bytes) y de 1,76 MB (22 sectores). Los
  sistemas de AmigaDOS: OFS y FFS, con sus variantes internacional y de caché de directorios, y los nombres largos
  de OS 3.2 si hay especificación pública. El bloque de arranque dice cuál es.
- **HDF:** con tabla de particiones del Amiga (RDB: `RDSK` en los primeros bloques, con sus `PART`) o sin ella (una
  sola partición desde el bloque 0, como los "hardfiles" de los emuladores; su geometría se deduce buscando el
  bloque raíz). Cada partición, por su tipo de DOS: FFS y OFS primero, después SFS y PFS3 (S5). El código del sistema
  de archivos que trae la RDB (`FSHD` con `LSEG`) es de 68000: no se ejecuta, se usa el controlador propio de ese
  tipo.
- **`trackdisk.device` nativo** sobre el ADF montado en `DF0:`: lectura y escritura por sectores; la lectura en bruto
  (MFM) se genera al vuelo para las pistas de AmigaDOS. Los discos con formato propio (protecciones, cargadores raros)
  no caben en un ADF y van a la máquina entera (S18).

### Dónde vive

- Un servicio por imagen montada, como un "handler" del Amiga (S5: cada sistema de archivos es un proceso aparte). Lee
  la imagen por el servicio de archivos, como cualquier fichero, y da el volumen. Si una imagen dañada lo tumba, cae
  ese servicio y nada más (S2, S12).
- Escrito desde especificaciones públicas: la documentación de AmigaDOS y la FAQ del formato ADF (Laurent Clévy). AROS
  sirve de referencia (es su licencia, APL). Las herramientas GPL (amitools, UAE) no se copian; amitools se usa solo
  como oráculo en las pruebas, ejecutándolo. La licencia de la documentación de PFS3: sin revisar.

## Cómo se prueba

- **Oráculo, amitools (`xdftool` y `rdbtool`)**, en un entorno aparte: crea ADF y HDF con ficheros conocidos (OFS,
  FFS, internacional, caché de directorios; con RDB y sin ella); DashOS los monta y se comparan listados y contenidos.
  Después, al revés: DashOS escribe y guarda, y `xdftool` valida la imagen y lee lo escrito.
- **Imágenes reales del usuario** (sus ADF de Workbench y juegos, sus HDF de Amiberry): se montan de solo lectura y se
  compara el listado con el de amitools. No van al repositorio.
- **Copia al escribir:** escribir, tirar los cambios y ver que la imagen no cambió ni un byte; escribir, guardar y
  validar con `xdftool`.
- **RAD:** en QEMU, escribir, reinicio en caliente, leer lo mismo; dañar un bloque de la zona a propósito y ver que el
  arranque lo detecta y carga la copia de la tarjeta; quitar la corriente (parar QEMU) y recuperar desde la copia.

## Fases

| Fase | Qué | Prueba |
|------|-----|--------|
| 1 | ADF de solo lectura (OFS y FFS, 880 KB y 1,76 MB) como volumen | Imágenes de `xdftool`, listados y contenidos |
| 2 | Escribir en ADF con la capa de cambios; guardar y tirar | `xdftool` valida lo guardado; tirar no cambia nada |
| 3 | HDF con RDB y sin ella, FFS; "meter en DF0:" en la caja Amiga | `rdbtool` y `xdftool`; un programa Amiga lee de DF0: |
| 4 | RAD: en memoria reservada y reinicio en caliente propio | Escribir, reiniciar, leer; daño detectado |
| 5 | RAD: por usuario, cifrado, con copia a la tarjeta | Parar QEMU y recuperar; otro usuario no lo lee |
| 6 | SFS y PFS3; `trackdisk.device` nativo con MFM al vuelo | Imágenes del usuario; un programa que lee en bruto |
| 7 | En la placa: si la RAM aguanta el reinicio del firmware | Solo informa: decide si RAD: lo aguanta en la Pi |

## Lo que queda abierto

- El tamaño de RAD: por defecto, y si se reparte entre usuarios o cada uno pide el suyo.
- Cada cuánto se copia RAD: a la tarjeta por defecto (o solo al apagar), pensando en el desgaste de la tarjeta (FS11).
- Si los nombres largos de OS 3.2 (DOS\6 y DOS\7) tienen especificación pública.
- La licencia de la documentación de PFS3.

## Cómo encaja

- Con S4b: `RAD:` junto a `RAM:`; `IMG0:` y `DF0:` para las imágenes.
- Con S5 y FS10: RAD: es un disco DashFS con un volumen cifrado por usuario; los sistemas del Amiga, procesos aparte.
- Con S6 y S14: el volumen de RAD: de cada usuario se abre al entrar, con su llave.
- Con S14: el reinicio en caliente propio, distinto del reinicio por el firmware.
- Con S18: las mismas imágenes para la máquina entera; las que no son AmigaDOS van allí.
- Con FS11: la copia de RAD: a la tarjeta cuenta para su desgaste.
