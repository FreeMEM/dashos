# S12b. Controladores de Linux con LKL, y lo nuevo de red

Del 2026-11-10. Recomendaciones aceptadas por el usuario el mismo día. Concreta la decisión del 2026-11-07 de S12
(controladores de Linux en procesos aparte, en la zona GPL) y añade a S13 lo que hace falta para el Wi-Fi. Es diseño:
no hay código.

> **Sustituido en lo de LKL (decidido por el usuario el 2026-11-16, S36).** Los controladores de Linux van en
> máquinas virtuales: los dominios de controladores de [S35](S35-controladores-de-linux.md). LKL se retira. Todo lo que
> este documento dice de LKL (la opción B, "Recomendación aceptada", lo que pide al kernel, el Wi-Fi por el proceso
> LKL, las fases L1 y L2 y la decisión 1) queda como historia y no se hace. **Lo de red sigue valiendo**: SACK y las
> marcas de tiempo si hacen falta, y `amissl.library` nativa. Los hilos en los procesos también siguen haciendo
> falta, pero ya no por LKL. El Wi-Fi de la Pi sigue por decidir con la placa delante (S35).

## El problema

La decisión de S12 era imitar, para cada controlador de Linux, las partes del kernel que usa (memoria, DMA,
interrupciones, temporizadores, colas de trabajo, PCI, USB), como DDE en Genode. Funciona, pero esa capa se escribe
otra vez para cada familia (red, Wi-Fi con su pila, USB, sonido con ALSA), y cada familia arrastra mucho del kernel.

## Las opciones

**A. Una capa por familia (DDE).** Lo decidido en S12. Cada controlador, solo con lo que usa.
- Cuesta: una capa grande por familia; el Wi-Fi arrastra `cfg80211` y a menudo `mac80211`.

**B. LKL: Linux como biblioteca.** El kernel de Linux compilado como un programa, con una arquitectura propia
(`arch/lkl`) que pide a quien lo aloja hilos, memoria, relojes y semáforos. Trae de una vez todos los controladores y
sus pilas (Wi-Fi, Bluetooth, USB, ALSA). GPL-2.0.
- Gana: la capa es una sola y pequeña (lo que pide `arch/lkl`), y vale para todas las familias.
- Cuesta: un proceso grande (megas de código), y necesita hilos en los procesos de DashOS, que aún no hay.

**C. Rump kernels de NetBSD.** Lo mismo con NetBSD, licencia BSD (se podría mezclar con MPL).
- Cuesta: muchos menos controladores que Linux, y no tiene el del Wi-Fi de la Pi.

## Recomendación aceptada: B, LKL

> Retirada el 2026-11-16: vale S35. Se deja el texto como estaba.

- **Un proceso "linux"** en la zona GPL (`AGENTS.md` regla 7), con LKL y solo los controladores que hacen falta. Puede
  haber varios, uno por aparato, para que un fallo no tumbe los demás.
- **Lo que le da DashOS**: hilos, memoria (también DMA, con su permiso), temporizadores, interrupciones y registros
  del aparato, por llaves, como a cualquier controlador (S12).
- **Cómo habla con DashOS**: por canales y anillos, nunca por llamadas directas. Para la red, un controlador más del
  servicio de red (S13): paquetes por el mismo anillo que la tarjeta con cable. Para el sonido, una salida más del
  grafo (S19b). Para el Bluetooth, lo mismo según el perfil.
- **Primero el Wi-Fi de la Pi** (el CYW43455 por SDIO, con su firmware cerrado, que se puede repartir), y después
  Bluetooth y los aparatos USB raros.
- **Seguridad**: el proceso "linux" es un controlador con sus llaves y nada más (S6b): si cae, cae su aparato.

## Lo que hace falta antes

- **Hilos en los procesos** (fase 8 de `docs/REVISION_ARQUITECTURA.md`): LKL corre los hilos del kernel de Linux
  como hilos del proceso. Es la dependencia que manda.
- **Memoria por demanda** o, al menos, objetos de memoria grandes: LKL reserva su propia memoria al arrancar.
- **Aviso de muerte (K7)**, para que el servicio de red sepa que el controlador de Wi-Fi cayó y lo relance (E6).

## Red: lo que se añade a S13

S13 sigue valiendo (smoltcp, rustls, cortafuegos, `bsdsocket.library` nativa). Se añade:

1. **Wi-Fi por el proceso LKL** de arriba. La gestión de redes y claves, en el servicio de Wi-Fi de S13.
2. **SACK y marcas de tiempo de TCP** en smoltcp si al medir en la Pi hacen falta: con Wi-Fi se pierden paquetes.
3. **`amissl.library` nativa antes de lo previsto** en S13, sobre rustls: Tachyon e Inferencia usan TLS todo el rato,
   y AmiSSL en el JIT va lento.

## Fases

| Fase | Qué | Prueba |
|---|---|---|
| L0 | Hilos en los procesos (fase 8 del informe) | Pruebas del kernel |
| L1 | LKL en DashOS: la arquitectura `lkl` sobre hilos, memoria y relojes de DashOS; arranca sin aparatos | Un proceso "linux" en QEMU que dice su versión y monta un disco en memoria |
| L2 | Red virtio dentro de LKL, como controlador del servicio de red | QEMU `virt` con `virtio-net`: DHCP y un ping |
| L3 | Wi-Fi de la Pi: SDIO, `brcmfmac` y su firmware | Pi 4 de verdad: conectar a una red WPA2 |
| L4 | Bluetooth y USB raros, según se pidan | — |
| L5 | `amissl.library` nativa sobre rustls | Tachyon descarga por HTTPS contra la de 68k |

## Decisiones

Aceptadas por el usuario el 2026-11-10:

1. LKL en vez de una capa por familia, en la zona GPL.
2. Orden: después de la revisión, de Wayland con el puente y del sonido de estudio (S19b). Empieza por los hilos.

## Cómo encaja

- S1 y S16: un controlador es un proceso con llaves; el kernel no crece.
- S6 y S6b: el proceso "linux" es un controlador sin más poderes.
- S7: la caja Linux es otra cosa (una máquina virtual para programas); LKL es solo para controladores.
- S12: concreta la decisión del 2026-11-07.
- S13: el Wi-Fi y las mejoras de TCP y TLS.
- S19b: el sonido por controladores de Linux, si alguna tarjeta lo necesita.
