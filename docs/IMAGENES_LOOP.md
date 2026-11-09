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
| P1 | `audio.device` nativo: canales, reparto con prioridad, `CMD_WRITE` con período, volumen y ciclos | En el PC y en el WAV de QEMU | |

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
