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
- **Lo que falta se ofrece, no se explica** (S36, regla 8). Si al abrir un fichero no hay con qué decodificarlo, sale
  un aviso en llano con un solo botón: "Instalar lo que falta" si hay un paquete que lo da, o "Abrirlo con el
  reproductor de Linux" si no. El usuario no tiene que saber qué es un códec; el nombre del formato va en "Detalles".
  Qué formatos van de serie no cambia con esto.

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

### Decidido el 2026-11-15: la gráfica, compartida a través de DashOS

La idea es la de WSL2: la gráfica la tiene quien tiene su controlador, y la caja Linux la usa por un puente, sin
poseerla. En WSL2, un controlador paravirtual de Linux (`dxgkrnl`) manda cada orden por el hipervisor al controlador
real de Windows, y el vídeo va por VA-API encima. El equivalente abierto es virtio-gpu (virgl para OpenGL, Venus para
Vulkan, contexto nativo como en ChromeOS) y virtio-video para decodificar. El puente no ahorra el controlador: solo
lo comparte. Así queda en cada máquina:

- **Raspberry Pi**: DashOS tiene sus controladores de pantalla, GPU y decodificador (S9, S21). La caja Linux pinta y
  decodifica a través de DashOS por virtio-gpu y virtio-video: manda el vídeo comprimido y DashOS lo decodifica.
- **PC y ARM que no son la Pi: lo manda S34** (`docs/diseño/S34-grafica-compartida.md`, rama `x86`; decidido por el
  usuario el 2026-11-15). En resumen: un dominio de gráficos (máquina virtual con Linux mínimo y los controladores del
  fabricante, en la zona GPL) lleva las gráficas por pase con IOMMU (SMMU en ARM); DashOS y la caja Linux son clientes
  suyos (contexto nativo o Venus, sin copia). Con dos gráficas, dos dominios: el de pantalla (integrada, el monitor
  enchufado ahí, el compositor y guardian) y el de cómputo (dedicada). La caja Linux sigue siendo máquina virtual
  aparte; CUDA llega por reenvío de la interfaz del controlador de Nvidia con lista blanca (como WSL2 o el nvproxy de
  gVisor), y mientras tanto con la dedicada pasada entera cuando la usa. El dominio, desde 4 GB y apagable en Prefs;
  con menos, pantalla del firmware y CPU. Requisito: IOMMU (DashOS x86 aún no lo tiene).
- Ejemplos de la casa: pegaso (i5-8500, UHD 630 y RTX 3050): pantalla en la UHD 630 con el monitor ahí, cómputo en la
  RTX (su grupo de IOMMU solo lleva la gráfica, su audio y el puerto PCIe); shiva (Ryzen 9 9955HX con Radeon 610M, y
  Arc A310 sola en su grupo; la Arc pide el firmware GuC/HuC y va mejor con Resizable BAR, sin comprobar en shiva).
- **Por CPU, siempre**: lo libre (AV1, VP9) cabe en cualquier PC moderno, también en 4K.

Sala limpia: los controladores de Linux corren dentro de la caja Linux o de la zona GPL, nunca copiados a DashOS.

## Cómo se mira por dónde se decodifica

Cada clase elige sola entre el decodificador de la máquina y la CPU. Para que se pueda ver y cambiar sin leer el
código (S0):

- Una orden de Shell lista las clases de DataTypes y el camino que usa cada una (máquina o CPU). Su nombre, por
  diseñar al hacerlo.
- Un ajuste de texto en `ENVARC:` fuerza la CPU, para depurar. Se cambia en Ajustes o con la orden `Ajuste`.
- Al abrir un fichero queda una línea en `Estado:Registros/` (S6) con el formato y el camino elegido.

## Cómo se ve en el escritorio

Los cuadros decodificados van como plano del compositor (planos del HVS en la Pi, S9; superficie con copia directa
en PC), sin pasar por la CPU cuando el decodificador los deja en memoria de la gráfica.

## Orden propuesto

1. Sonido libre como DataTypes (sin vídeo): Opus, Vorbis, FLAC, MP3, WAV, AIFF, 8SVX, MOD.
2. AV1 y VP9 por CPU, con animation.datatype para la caja Amiga.
3. Decodificador de la Pi (S21, H5) con el puente virtio-gpu y virtio-video para la caja Linux; en x86, IOMMU y
   luego la gráfica según la máquina (arriba).
4. Códecs extra como paquete (decisión del usuario).
5. Controlador propio de Intel, si hace falta.

## Lo que queda abierto

1. ¿Códecs extra (H.264, HEVC, AAC por software) como paquete de Hangar? Decisión legal del usuario.
2. VP9 y Opus: C con licencia BSD o Rust.

## Cómo encaja

S4b (`DEVS:DataTypes`), S9 (planos del compositor), S19 (sonido), S21 (decodificadores de las placas), S25 (GPU de la
caja Linux), S30 (x86), S8 (paquete de códecs extra).
