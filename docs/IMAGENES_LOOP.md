# Imágenes del Amiga (S20) y audio.device (S19, fase 2)

Plan y bitácora. Diseños: `docs/diseño/S20-rad-adf-hdf.md` y `docs/diseño/S19-sonido.md`. Pedido por el usuario el
2026-11-09 ("continúa por S19 fase 2, S20 fase 1").

S19 fase 2 (`audio.device` y AHI en la caja Amiga) necesita la caja, que aún no existe en DashOS (H4: exec nativo,
tareas, señales y puertos). Por eso va primero S20, fase 1, y de S19 la parte nativa: los cuatro canales de Paula
sobre el servicio de sonido, con las órdenes de `audio.device`; el enganche con los programas 68k, con H4.

| Paso | Qué | Prueba | Estado |
|------|-----|--------|--------|
| I1 | Biblioteca `amigados`: OFS y FFS, normal, internacional y caché de directorios, 880 KB y 1,76 MB, solo leer | `tools/adf_oraculo.py`: imágenes de `xdftool`, árbol, contenidos, bits, comentarios, libres; dañadas no caen | hecho |
| I2 | Servicio `imagen`: un ADF como volumen de solo lectura, con el protocolo de archivos | `adf-prueba` en virt: listar, leer, buscar sin mayúsculas, rechazos, comprobar | hecho en virt |
| P1 | `audio.device` nativo: canales, reparto con precedencia y cerrojos, `CMD_WRITE` con período, volumen y ciclos, `FINISH`, `PERVOL`, `WAITCYCLE`, parar, vaciar, abortar | 8 pruebas en el PC; `make sonido-paula`: el WAV de QEMU | hecho, también en virt |
| P2 | AHI nativo: la biblioteca (canales con sonidos que se repiten, cambios con y sin `AHISF_IMM`, hacia atrás, jugador) y `ahi.device` (escrituras seguidas por unidad) | 6 pruebas en el PC | hecho en el PC |
| P3 | Enganchar los dos a los programas 68k (con H4), y compararlos con FS-UAE | Programas Amiga suenan igual | espera a H4 |

## Bitácora

### 2026-11-09

- I1: `amigados/src/lib.rs`, desde la FAQ del ADF de Laurent Clévy. Comprueba todo lo que lee (suma de cada bloque,
  tipo, su número, números dentro del disco, largos) y corta las cadenas que dan más vueltas que bloques hay.
  `tools/adf_oraculo.py` hace con `xdftool` (amitools, en `~/.cache/amitools-venv`, solo ejecutado) ocho imágenes:
  OFS, FFS, OFS y FFS internacionales, OFS y FFS con caché de directorios, y OFS y FFS internacional de 1,76 MB. Cada
  una con 171 entradas: tamaños en los bordes de los bloques (0, 1, 487 a 489, 511 a 513, 976, 1024), uno de 100 KB
  (300 KB en las grandes) que necesita bloques de extensión, cajones anidados, 150 ficheros en un cajón (cadenas de
  hash), nombres de Latin-1 y de 30 letras, comentarios y protecciones. Las ocho, bien a la primera. Para fiarse del
  oráculo, se probó con un fallo puesto a propósito (OFS leyendo un byte de menos): lo vio. Con 300 imágenes dañadas
  al azar, ninguna caída.
- I2: `usuario/src/bin/imagen.rs` y `usuario/src/bin/adf_prueba.rs`; el protocolo, en `docs/referencia/servicios.md`
  (orden 27, montar). El disco de virt lleva `Imagenes/disquete.adf` (`kernel/pruebas/disquete.adf.gz`, 24 KB, hecho
  con `tools/adf_oraculo.py --disquete`). En virt: monta, lista, lee 97.890 bytes en 9.000 líneas, encuentra
  `cajón/ÑANDÚ.txt` por el hash internacional, rechaza escribir y la comprobación no ve problemas.
- P1: `audiodevice/src/lib.rs`, desde el capítulo "Audio Device" del ROM Kernel Reference Manual y `devices/audio.h`.
  Recibe peticiones con los campos de `IOAudio` y devuelve avisos (lo que sería `ReplyMsg`, y `ioa_WriteMsg`). Suena
  como Paula: muestras de 8 bits con signo por su volumen (0 a 64), cada una mantenida hasta la siguiente, al ritmo
  del reloj PAL entre el período; 0 y 3 a la izquierda, 1 y 2 a la derecha. Todo con enteros (coma fija 32.32): el
  servicio ya paga la coma flotante por software. Diferencia con el diseño: los cuatro canales salen ya mezclados en
  un flujo estéreo de 48 kHz, no un flujo por canal, porque el período cambia en cada escritura y la frecuencia de un
  flujo es fija. Sin comprobar contra el original: con qué error vuelven las escrituras de un canal robado y la que
  corta `ADCMD_FINISH`. Pruebas: asignar por combinaciones y precedencia (robar), esperar a que se suelte, cerrojo
  (avisa y la asignación espera al `FREE`), frecuencia y lado de cada canal (439,84 y 659,76 Hz, a 0,5 Hz), ciclos
  contados a ±1 cuadro, cola sin hueco, `FINISH` y `PERVOL` al final del ciclo y ya, `WAITCYCLE`, parar, vaciar,
  abortar y muestras de 2 bytes a período mínimo. Siete de ocho bien a la primera; la otra era la prueba (un seno pasa
  por cero, y lo tomaba por hueco).
- P1 en QEMU: `usuario/src/bin/paula_prueba.rs` (`make sonido-paula`, con la función `paula` del kernel en lugar de
  los dos tonos): 440 Hz a la izquierda 1 s; 660 Hz a la derecha sin fin, robado a los 0,5 s por una asignación de
  más precedencia, que pone 880 Hz otros 0,5 s. `tools/sonido_wav.py --paula` lo mide en el WAV.
- P2: `audiodevice/src/ahi.rs`, desde la guía del programador de AHI. La biblioteca: canales que repiten un tramo de
  un sonido (también hacia atrás), con frecuencia, volumen y paneo en 16.16; sin `AHISF_IMM` el cambio espera al
  final de la vuelta; `Empieza` en cada vuelta (`AHIA_SoundFunc`) y `Jugador` a su ritmo (`AHIA_PlayerFunc`);
  interpolación lineal. Tipos de muestra de 8, 16 y 32 bits, mono y estéreo, desde el big-endian del Amiga.
  `ahi.device`: una cola de escrituras por unidad que suenan seguidas, sin hueco (lo que sobra de la posición pasa a
  la siguiente a su frecuencia), y vuelven al acabar. Sin comprobar: la ley del paneo (aquí lineal) y cuándo sale el
  primer `Empieza`. Dos pruebas mal planteadas por mí (una vuelta acaba con su último cuadro, no con el siguiente; y
  44,1 kHz no es exacto en coma fija: se mide a ±1 cuadro).
- P1 en virt: bien a la primera. El robo vuelve a los 501 ms con -12 y las dos escrituras de 440 ciclos acaban a
  los 1.002 ms; el WAV, 440 Hz a la izquierda en 10 ventanas de 0,1 s y, a la derecha, 660 Hz en 5 y 880 Hz en 5,
  sin cruzarse; 0 vacíos. El comprobador, con el WAV de los otros tonos, dice MAL.
