# S33. Códecs de sonido y de vídeo

Borrador del 2026-11-15, pedido del usuario: "¿cómo vamos a resolver los códecs de vídeo y audio?", sin dejar fuera
x86-64 con gráficas AMD, Nvidia e Intel. Es diseño: no hay nada hecho salvo el servicio de sonido (S19) y lo que dice
S21 de los decodificadores de las placas.

## La idea: DataTypes, como en el Amiga

Cada formato es una clase de DataTypes (`DEVS:DataTypes`, S4b): sonido, animación (vídeo) e imagen. Un programa pide
"abre y reproduce esto" y no sabe de códecs. Las mismas clases sirven a los programas nativos y a los 68k de la caja
Amiga: `MultiView` o `AmigaAMP` ven los formatos nuevos sin cambiar. Por debajo, cada clase elige dónde decodifica:
el decodificador de la máquina si lo hay y sabe ese formato; si no, la CPU.

## Formatos, por su licencia

- **En `SYS:`, libres de patentes**: sonido Opus, Vorbis, FLAC, MP3 (patentes caducadas en 2017), WAV, AIFF, 8SVX y
  los MOD de tracker; vídeo AV1 y VP9 (y Theora), en Matroska/WebM, Ogg y MP4 como contenedor.
- **H.264, HEVC y AAC**: por el decodificador de la máquina, cuyo fabricante paga las patentes. Por software, no van
  en `SYS:`: un paquete opcional de "códecs extra" (como hacen Fedora y openSUSE). Si se distribuye y desde dónde, lo
  decide el usuario: es una cuestión legal.
- **Respaldo para todo**: la caja Linux con mpv y ffmpeg de su distribución.

## Por CPU

- Decodificadores en Rust con licencia compatible con MPL-2.0: symphonia (casi todo el sonido) y rav1d (AV1, port
  de dav1d). Piden la biblioteca estándar de Rust, que los programas de DashOS no tienen (`no_std` con `alloc`): hay
  que adaptarlos o darles una capa mínima (ficheros, hilos, reloj). VP9 y Opus: por decidir (libvpx y libopus son C
  con licencia BSD; o versiones en Rust).
- Con NEON en ARM y AVX2 en x86-64, AV1 a 1080p cabe en la CPU de una Pi 5 y de cualquier PC; 4K, en un PC.

## Por el decodificador de la máquina

| Máquina | Qué tiene | Cómo se llega |
|---|---|---|
| Pi 4 | H.264 y HEVC (hasta 4K) | Controlador propio (S21, H5) |
| Pi 5 | HEVC; H.264 no (CPU) | Controlador propio (S21, H5) |
| PC con Intel | Quick Sync: H.264, HEVC, VP9, AV1 (según generación) | Intel publica la documentación de sus gráficas: controlador propio posible, grande |
| PC con AMD | VCN: H.264, HEVC, VP9, AV1 (según generación) | Documentación parcial y firmware cerrado: muy grande en nativo |
| PC con Nvidia | NVDEC: H.264, HEVC, VP9, AV1 | Sin documentación pública del decodificador; firmware cerrado (GSP): no en nativo |

En x86-64 lo realista es por fases:

1. **CPU** para todo lo libre: un PC moderno lo decodifica sin esfuerzo.
2. **La gráfica entera para la caja Linux** (pasarla por el hipervisor, con su IOMMU, como VFIO): Linux usa sus
   propios controladores (i915/xe, amdgpu, nouveau o el de Nvidia) y VA-API o NVDEC, y DashOS recibe los cuadros
   como superficies de su compositor. Sirve para AMD y Nvidia sin escribir sus controladores.
3. **Controlador propio de Intel** (la documentación es pública) si hace falta vídeo acelerado en nativo.

Sala limpia: los controladores de Linux se pueden usar dentro de la caja Linux, pero no se copian a DashOS.

## Cómo se ve en el escritorio

Los cuadros decodificados van como plano del compositor (planos del HVS en la Pi, S9; superficie con copia directa
en PC), sin pasar por la CPU cuando el decodificador los deja en memoria de la gráfica.

## Orden propuesto

1. Sonido libre como DataTypes (sin vídeo): Opus, Vorbis, FLAC, MP3, WAV, AIFF, 8SVX, MOD.
2. AV1 y VP9 por CPU, con animation.datatype para la caja Amiga.
3. Decodificador de la Pi (S21, H5) y, en x86, pasar la gráfica a la caja Linux.
4. Códecs extra como paquete (decisión del usuario).
5. Controlador propio de Intel, si hace falta.

## Lo que queda abierto

1. ¿Códecs extra (H.264, HEVC, AAC por software) como paquete de Hangar? Decisión legal del usuario.
2. VP9 y Opus: C con licencia BSD o Rust.
3. En x86, ¿pasar la gráfica entera a la caja Linux deja a DashOS sin gráfica? Hace falta una segunda (la integrada
   para DashOS, la dedicada para Linux) o compartir por SR-IOV (Intel reciente). Con la rama `x86`.

## Cómo encaja

S4b (`DEVS:DataTypes`), S9 (planos del compositor), S19 (sonido), S21 (decodificadores de las placas), S25 (GPU de la
caja Linux), S30 (x86), S8 (paquete de códecs extra).
