# Bucle del sonido (S19, fase 1)

Orden del usuario (2026-11-09): tras el compositor Wayland y el kit de interfaz, el sonido; el pulido del kit, al
final. Diseño: `docs/diseño/S19-sonido.md`. Esta fase: el servicio "sonido" con mezcla y remuestreo, y el controlador
de virtio-sound en QEMU `virt`, probado con el WAV que escribe QEMU.

## Dónde está

- Copia de trabajo `.worktrees/sonido`, rama `sonido`, desde la del kit (`kit`).
- `sonido/`: la biblioteca de mezcla (no_std con alloc), probada en el PC.
- `usuario/src/bin/virtio_sonido.rs`: el controlador; `usuario/src/bin/sonido.rs`: el servicio.

## Decisiones tomadas por defecto (cambiables)

- **virtio-sound por PCI** (`virtio-sound-pci`), como el NVMe y el xHCI de `virt`. Escrito con la especificación
  virtio 1.2: transporte PCI (cap. 4.1) y dispositivo de sonido (cap. 5.14).
- **El controlador espera la interrupción del aparato** (desde A6; al principio miraba la cola con un temporizador).
- **Salida: 48 kHz, estéreo, 16 bits con signo.** Mezcla en coma flotante; período de 256 muestras (5,3 ms).
- **Remuestreo lineal** al principio. Uno mejor (polifásico) cuando haya prueba que lo pida.

## Pasos

| Paso | Qué | Prueba | Estado |
|------|-----|--------|--------|
| A1 | Biblioteca `sonido`: formatos (16, 32 bits, flotante; mono, estéreo), remuestreo, mezcla con volumen, fundido al quitar un flujo | En el PC: nivel, frecuencia de un tono tras remuestrear, sin chasquido al quitar | hecho |
| A2 | El kernel encuentra virtio-sound (fabricante 0x1af4, aparato 0x1059) y da su BAR de virtio | El controlador ve el aparato en QEMU | hecho |
| A3 | Controlador virtio-sound: colas, PCM de salida a 48 kHz, suena un tono | El WAV de QEMU: frecuencia y nivel del tono, sin huecos | hecho en virt |
| A4 | El servicio "sonido": flujos con anillo en memoria compartida y canal; dos programas suenan a la vez | El WAV: los dos tonos mezclados, con su volumen | hecho en virt |
| A5 | Matar un programa a mitad: su flujo se quita con fundido | El WAV: sin chasquido, el otro sigue | hecho en virt |
| A6 | Cortes con los cuatro núcleos ocupados: períodos no entregados a tiempo | La cuenta, en la salida; raspi4b sigue bien | hecho: 0 vacíos (ver bitácora) |

## Bitácora

### 2026-11-09

- A1: `sonido/src/lib.rs`. `Mezclador` con flujos en 16 o 32 bits o flotante, mono o estéreo, de 8 a 192 kHz;
  remuestreo lineal; volumen por lado (sirve de paneo) en rampa de un período; un flujo nuevo sube desde cero y uno
  cerrado sale con 5 ms de fundido gastando su cola; faltas contadas solo tras la primera muestra; reajuste del ritmo
  en partes por millón; cola de un segundo como mucho. 8 pruebas: formatos, tono de 1 kHz de 44,1 a 48 kHz (frecuencia
  a 0,5 Hz, pico, cuadros gastados), mezcla con volumen y paneo, cerrar sin escalón, volumen sin escalón, faltas,
  saturar a 16 bits y ajuste de reloj. Dos de mis cuentas en las pruebas estaban mal (períodos enteros; el primer
  cuadro sale del silencio inicial).
- A2: `kernel/src/inicio.rs`. `aparato_pci` busca por clase o por fabricante y aparato, y mide la BAR que se le pida
  (la 4 para virtio); devuelve también la física del espacio de configuración del aparato, que se da al controlador
  (4 KB) para leer sus capacidades. La MSI-X solo con la tabla en la BAR 0: virtio-sound va por INTx (y el controlador
  no la usa). Las BAR de EDK2 cambian de sitio al añadir el aparato: la línea esperada del xHCI ya no lleva la
  dirección.
- A3: `usuario/src/bin/virtio_sonido.rs`, virtio 1.2 desde la especificación: capacidades en la BAR 4, versión 1,
  colas partidas de 16 (control y salida), `PCM_INFO`, parámetros (estéreo, 16 bits, 48 kHz, períodos de 256
  cuadros, búfer de 8), preparar, empezar; cada búfer en dos descriptores (cabecera con datos, y estado). Mira la
  cola cada 2 ms. En QEMU: 375 períodos en 2.005 ms, sin errores. `tools/sonido_wav.py` comprueba el WAV de QEMU
  (`-audiodev wav`): 1,996 s a 1.000,00 Hz, pico 16.383, sin huecos. QEMU no escribe los tamaños de la cabecera del
  WAV cuando se para con una señal: el comprobador lee la cabecera a mano. El controlador cuenta 6 "posibles faltas"
  (todos los búferes devueltos entre dos miradas), pero el WAV no tiene huecos: QEMU devuelve los búferes a ráfagas.
  La cuenta de faltas buena será la de A6.
- A4: el servicio `usuario/src/bin/sonido.rs` y los programas `tono-a` y `tono-b` (`usuario/src/sonido_cliente.rs`);
  protocolo en `docs/referencia/servicios.md`. El reloj lo marca el aparato: el controlador da al servicio una memoria
  con 8 ranuras de un período y le manda un tic por cada búfer que devuelve el aparato; el servicio mezcla hasta tener
  4 por delante. Cada programa escribe en su anillo y el servicio lo despierta por un canal del programa cuando baja de
  la mitad. `tono-a`: 1 kHz a 0,25, mono de 16 bits a 48 kHz; `tono-b`: 600 Hz a 0,4 con volumen 0,5, estéreo de
  coma flotante a 44,1 kHz. `tools/sonido_wav.py` mide cada tono en ventanas de 0,1 s, huecos y saltos entre muestras.
- A5: `tono-a` escribe 1,5 s y un cuarto de ciclo y falla a propósito; el kernel lo mata sin `CERRAR`. El servicio lo
  nota al despertarlo (su canal está cerrado) y lo quita con el fundido de 5 ms, a mitad de un ciclo, sin chasquido.
  Lo que quedaba en su anillo (unos 85 ms) no suena: lo nota cuando el anillo baja de la mitad.
- A6, la medida y el arreglo:
  - Primera prueba de virt con A4 y A5: 22 a 30 períodos vacíos en 2 s, casi todos en el primer medio segundo, y el
    WAV con huecos de 5 a 21 ms. Con cuatro programas girando en los núcleos (`make sonido-carga`): 0 vacíos. No era
    la carga de CPU.
  - Poner el servicio y el controlador en la clase de plazo (llamada 28, `plazo`, nueva; el kernel solo tenía plazos
    para sus hilos) no lo arregló: 22 vacíos.
  - Midiendo las paradas: el controlador se quedaba 20 a 50 ms sin correr cada medio segundo (el ritmo de las teclas
    de la prueba), y mezclar un tic costaba 4 a 8 ms (coma flotante por software, en QEMU).
  - La causa: `dormir_us` va con los temporizadores del kernel, en tics de 10 ms, y los entrega el repartidor del
    núcleo 0, un hilo de la clase justa que espera su turno detrás de lo que corra allí. Las interrupciones de
    aparatos pasan por el mismo repartidor.
  - El arreglo: el repartidor va en la clase de plazo (300 µs cada ms; si no se admite, en la justa); un hilo de
    plazo que despierta en su propio núcleo no espera al tic (aviso SGI a sí mismo); el controlador espera la
    interrupción del aparato (INTx, leyendo su ISR) en vez de dormir; el servicio, con 3 ms cada 5,3 ms.
  - Con eso, virt: 0 vacíos, ninguna parada de más de 15 ms, el WAV sin huecos ni chasquidos.
  - Pendiente: quién puede pedir la clase de plazo (S6; hoy cualquiera, acotado al 80 % de cada núcleo). Mezclar en
    coma flotante por software es caro: enteros en coma fija o la FPU (los programas se compilan sin ella) cuando
    haya más flujos. La resolución de los temporizadores sigue en 10 ms.
