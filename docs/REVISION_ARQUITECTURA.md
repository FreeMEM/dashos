# Revisión de la arquitectura de DashOS (2026-11-10)

Informe de mejoras para que otra sesión las implemente. Lo pidió Fran: mirar el diseño de datos, las medidas y las
pruebas de velocidad, y decir cómo hacer DashOS más rápido, más seguro y más estable, con el sistema de archivos en
primer lugar.

Esto es una revisión, no un cambio: no se tocó código. Estado revisado: rama `multitarea`, commit 5ad96b1.

## 0. Cómo leer y usar este informe

**Certeza de cada punto.** Cada hallazgo lleva una marca:

- **[C]** comprobado: lo leyeron dos veces, una revisión y después quien firma, en el código.
- **[L]** leído una vez por una revisión, sin segunda lectura.
- **[S]** sospecha: sale de deducir o de una cifra de la bitácora. Hay que medir o provocar el caso antes de arreglar.

Nada de este informe se compiló ni se ejecutó. Las ganancias son estimaciones salvo que se diga que son medidas.

**Números de línea.** Son del árbol de trabajo. En `kernel/src/base/planificador.rs` y `kernel/src/base/excepciones.rs`
había una sonda sin commit: sus líneas van hasta 4 por encima de las de 5ad96b1. Manda el nombre de la función.

**Reglas del repo que debe respetar quien implemente** (están en `AGENTS.md`; aquí lo que más pesa):

- `unsafe` solo en `kernel/src/base/`.
- Sala limpia: no se lee Linux ni desensamblados para implementar. vAmiga (MPL) se puede leer; UAE, WinUAE y
  Amiberry (GPL) no. `archivos/redoxfs` es un fork local y se puede cambiar; nada hacia RedoxFS sin permiso.
- Ni ROMs, ni imágenes de sistema, ni kernels, DTB o firmware en el repo.
- Toda ejecución de emulador bajo `flock /run/user/1000/dash-emu-red.lock`. No se editan fuentes mientras corre una
  compilación o una prueba.
- Cada paso: medir antes y después (Rust, C y Dash donde toque), `make prueba` y `make arranque-virt` en `kernel/`,
  números a la bitácora y a `docs/medidas/`, commit local. Nada hacia fuera sin permiso de Fran.
- Cada llamada o protocolo que cambie se documenta en `docs/referencia/kernel.md` o `docs/referencia/servicios.md` en
  el mismo commit.
- Los cambios de Dash los hacen las sesiones de Dash, no esta.

**Cómo se mide.** `make -C kernel medir VUELTAS=n` (multitarea, `target/medir.json`), `make -C kernel medir-fs`
(ficheros), `python3 tools/medir.py comparar a.json b.json`. Linux: `tools/medir.py linux` y `linux-fs`. Lo guardado
está en `docs/medidas/`.

## 1. Dónde se va el tiempo

Medidas en QEMU (Raspberry Pi OS 6.18 en raspi4b para multitarea y memoria; Debian 13 con ext4 en virt para ficheros).

| Medida | DashOS | Linux |
|---|---|---|
| Ida y vuelta de un mensaje, sin carga, entre núcleos | 62 µs | 34 µs |
| Ida y vuelta, mismo núcleo | 77 µs | 83 µs |
| Ida y vuelta con 4 calculando | 98 a 193 µs según la vuelta | 153 µs |
| Datos de un fichero (stat) | 14.276/s | 27.084/s |
| Leer 4 KB (abrir, leer, cerrar) | 3.028/s | 8.984/s |
| Leer 8 MB ya leídos | 407 MB/s | 1.652 MB/s |
| Leer 8 MB, primera vez | 300 MB/s | 401 MB/s |
| Crear 4 KB, guardado | 375/s | 442/s |
| Crear 4 KB sin fsync | 388/s | 3.107/s |
| Borrar, guardado al final | 1.932/s | 5.397/s |
| Escribir 8 MB, guardado al final | 78,6 MB/s | 109,5 MB/s |
| Listar 500 | 405/s | 165/s |
| RAM al arrancar | 9,3 MB | 89,6 MB |
| Memoria por proceso | 122 KB | 538 KB |

Lo que explican el código y el perfil (`docs/medidas/fs-f11.json`):

1. **El mensaje manda en todo lo pequeño.** Una ida y vuelta son 4 entradas al kernel (escribir y leer en cada lado).
   `stat` es un viaje: 70 µs. Leer 4 KB son tres viajes: unos 200 de sus 330 µs. Leer 8 MB ya leídos son 128 viajes
   por la ventana de 64 KB: 8 a 10 ms de los 20 que tarda.
2. **Dentro del mensaje, lo que pesa es entrar y salir del kernel.** Cada llamada cambia TTBR0 dos veces (PAN por
   software). QEMU vacía su TLB en cada cambio. Un experimento de la bitácora (`m9-asid.json`, no seguro) bajó la ida
   y vuelta de 91 a 26 µs sin ese cambio. Que en el mismo núcleo cueste más (77) que entre núcleos (62) dice lo mismo:
   el coste está en la llamada, no en el aviso. En una Cortex-A72 real ese cambio no vacía nada. **Parte de la
   distancia con Linux puede ser del emulador, y hoy no se sabe cuánta.**
3. **Cerrar un fichero cambiado guarda en el disco siempre.** Por eso crear con y sin fsync da lo mismo, y sin fsync
   Linux va 8 veces por delante.
4. **Leer mucho copia 3 o 4 veces** (caché, registro, ventana, cliente) y cada lectura abre una transacción que clona
   el asignador entero.
5. **La medida con carga no es repetible.** Tres vueltas dieron 115, 183 y 137 µs. Nada se fija a un núcleo y el
   dato es una media, no una mediana.

De ahí salen tres palancas, de más barata a más cara:

- **Menos viajes** (órdenes combinadas, ventana mayor, no guardar al cerrar). Es código de usuario, pequeño, y se
  mide ya en QEMU.
- **Viajes más baratos sin cambiar la ABI** (menos cerrojos, sin montón por mensaje).
- **Llamada de ida y vuelta con paso directo** (ABI nueva). Antes hay que medir en placa real.

En estabilidad y seguridad, lo grave no es de velocidad: **un cliente, un disco o un USB malos pueden tumbar un
servicio entero, y nadie lo relanza.**

## 2. Orden de implantación

Cada fase acaba con medida, pruebas y commit. Dentro de una fase, los puntos son independientes salvo que se diga.

| Fase | Qué | Puntos | Tamaño |
|---|---|---|---|
| 0 | Banco fiable y línea base en placa real | B1 a B5 | pequeño |
| 1 | Arreglos pequeños y seguros de estabilidad y seguridad | E1 a E5, K8 a K11, S3, S4, S9 | pequeño |
| 2 | Archivos, lo rápido: menos viajes y no guardar al cerrar | A1 a A6 | pequeño y medio |
| 3 | Camino caliente del kernel, sin cambiar la ABI | K1 a K5 | medio |
| 4 | ABI nueva: llamada de ida y vuelta, plazo y aviso de muerte | K6, K7 | grande |
| 5 | Servicios: biblioteca común y supervisión | E6 a E9 | grande |
| 6 | Kernel sin pánicos y con trabajo acotado | S1, S2, S5, S6 | medio |
| 7 | Archivos, lo grande | A7 a A14 | grande |
| 8 | Capacidades que faltan | hilos, memoria por demanda, S6 | grande |

Las fases 2 y 3 no dependen una de otra. La 4 depende de la 0 (B5) y de la 3. La 5 depende de K7. A7 (caché
compartida) gana mucho con K6.

**Decisiones que son de Fran** (no las toma quien implementa):

- **A1**: que cerrar un fichero ya no lo guarde en el disco. Cambia qué se pierde si se va la luz.
- **K6 y K7**: números de llamada nuevos (32 en adelante).
- **S3**: quién puede pedir la clase de plazo.
- **A13**: permisos por programa en el servicio de archivos (es parte de S6).

**Decidido por Fran el 2026-11-10:** A1 sí, junto con A2 (la orden 27, "guardar", sigue siendo durable; sin opción
por volumen). K7 sí, con los números 32 en adelante. K6 espera a medir en la Pi real (B5); se reservan sus números.
S3: la clase de plazo solo con una llave que reparte `inicio`, para el compositor y el sonido. A13: ahora lo barato
(derecho "administrar" y contraseña fuera del mensaje); el resto, con `diseño/S6b-usuarios-y-permisos.md`.

## 3. Banco de medidas (fase 0)

Va primero: sin esto no se distingue una mejora del 10 % del ruido.

**B1. Fijar todo a un núcleo en la ida y vuelta con carga** [C]
- Dónde: `tools/medir.py:64` (la clave es lenguaje, juntos y carga) y `usuario/src/bin/medir.rs` (banco de ida y
  vuelta).
- Qué pasa: con `juntos` sin fijar, ni eco, ni ping, ni las 4 cargas van atados. El resultado depende de dónde
  caigan: 98 µs con eco en un núcleo sin carga, 118 a 256 en los demás (`docs/MULTITAREA_LOOP.md`, M10).
- Arreglo: una carga por núcleo, eco y ping fijados, y el reparto dentro de la clave de la medida. Las cargas duran
  hasta "fin", no 4 s fijos.

**B2. Mediana y p99 por ronda, no media** [L]
- Dónde: `usuario/src/bin/medir.rs` (calcula `us * 1000 / n` sobre 5.000 rondas).
- Qué pasa: unas pocas esperas de varios milisegundos hinchan la media.
- Arreglo: guardar el tiempo de cada ronda (o un histograma) y dar mediana, p99 y máximo. Una tanda de calentamiento
  antes.

**B3. Más arranques y dispersión a la vista** [C]
- Dónde: `kernel/Makefile:90` (`VUELTAS ?= 3`).
- Arreglo: 7 arranques por defecto en las medidas que se van a comparar y guardar mínimo, mediana y máximo.
  `comparar` debe decir "dentro del ruido" cuando la diferencia es menor que la dispersión.

**B4. QEMU sin vecinos** [S]
- Qué pasa: hay otras sesiones con emuladores en el mismo PC. Con 4 núcleos emulados ocupados, el planificador del
  PC entra en la medida.
- Arreglo: fijar los hilos de QEMU a núcleos libres (`taskset`) y apuntar en el JSON la carga del PC al medir.

**B5. Línea base en Raspberry Pi real** [S]
- Qué pasa: no hay ninguna medida en placa. La cuenta del cambio de ASID (punto 2 de la sección 1) decide si K6
  merece la pena y cuánto.
- Arreglo: `make -C kernel medir-pi3` existe. Las tarjetas están preparadas fuera del repo. Falta el cable serie;
  Fran avisa cuando esté. Medir DashOS y Linux en la misma placa y guardar los dos JSON.
- Mientras tanto: en QEMU `virt` se puede arrancar con una CPU que tenga PAN de hardware y medir un camino que no
  toque TTBR0 (ver K5). Da una cota de lo que se ganaría en una Pi 5.

**B6. Pruebas que faltan** [L]
- Dónde: `tools/prueba_qemu.sh`, `kernel/pruebas/*.txt`.
- Qué pasa: las pruebas buscan líneas esperadas en el camino feliz. No hay cliente hostil, ni muerte de un servicio,
  ni cola llena, ni falta de memoria, ni fallo de disco. `wayland/` y `sonido/` tienen pruebas de PC que ningún
  Makefile lanza. El plazo fijo en segundos falla con el PC cargado.
- Arreglo: un programa "hostil" en el arranque de pruebas (mensajes vacíos, 8 llaves por mensaje, cola llena), una
  prueba que mate un servicio, `cargo test` de `wayland/` y `sonido/` dentro de `make prueba`, y fin por marca en la
  salida en vez de por segundos. Cada arreglo de las secciones 6 y 7 entra con su prueba.

## 4. Sistema de archivos

Ficheros: `usuario/src/bin/archivos.rs` (servicio), `archivos/redoxfs/src/` (RedoxFS, fork local),
`usuario/src/bin/nvme.rs`, `usuario/src/bin/sd.rs`, `usuario/src/ficheros.rs` (cliente).

Cómo está hecho hoy: un proceso de un solo hilo atiende hasta 8 clientes por un canal, con llaves marcadas. Cada
cliente tiene una ventana compartida de 64 KB. Debajo, una caché de bloques de 4 KB (hasta 40 MB) y el servicio de
disco, con un anillo de 32 entradas. Las transacciones se confirman en memoria y van al disco al cerrar lo cambiado,
con VACIAR o GUARDAR, a los 8 MB pendientes o al segundo.

### Fase 2: lo rápido

**A1. Cerrar ya no guarda en el disco** [C] — decisión de Fran
- Dónde: `archivos.rs:863` (`if c.cambiadas.remove(&p.asa) { guardar(fs) }`).
- Qué pasa: cada cierre de un fichero cambiado llama a `fs.durable()` y espera el vaciado del disco (1,5 ms en
  QEMU). Crear sin fsync: 388/s frente a 3.107/s de Linux.
- Arreglo: CERRAR solo deja lo cambiado pendiente. Ya existen el plazo de 1 s (`PLAZO_US`), el tope de 8 MB
  (`MAX_SUCIO`) y las órdenes VACIAR (11) y GUARDAR (27) para quien quiera el dato en el disco ya.
- Qué cambia para el usuario: si se va la luz, se puede perder hasta el último segundo de ficheros cerrados. El disco
  queda entero igual (RedoxFS no reusa bloques soltados hasta guardar). Es lo que hace Linux. Si Fran prefiere lo de
  hoy, se deja como opción por volumen.
- Va junto con A2: sin él, un fallo del guardado diferido no llega a nadie.
- Ganancia estimada: crear sin fsync hacia 1.000 a 1.500/s (borrar sin guardar ya da 1.932/s).
- Esfuerzo: pequeño. Prueba: `medir-fs`, y `archivos/redoxfs/tests/power_cut.rs` ampliada al guardado diferido.

**A2. Los errores de guardado no se pierden** [C]
- Dónde: `archivos.rs:1297` y `:1374` (el fallo de `durable()` del temporizador solo se imprime); `:553`
  (`cerrar_nodo` ignora el error de `on_close_node`); `soltar` (`:1242`) cierra asas sin guardar.
- Qué pasa: el programa recibió BIEN y el dato no está en el disco.
- Arreglo: un estado de error pegado al volumen. Se devuelve en el siguiente GUARDAR, VACIAR o CERRAR de quien tenía
  cambios. Tras un fallo de escritura, el volumen pasa a solo lectura hasta comprobarlo.
- Esfuerzo: pequeño o medio. Prueba: un disco de pruebas que falle a la enésima escritura.

**A3. Leer un fichero entero en un viaje** [C]
- Dónde: `archivos.rs`, órdenes BUSCAR (`p.a == 1` ya da los datos en un viaje) y GUARDAR (27, ya crea con datos en un
  viaje); cliente en `usuario/src/ficheros.rs`.
- Qué pasa: abrir, leer y cerrar son tres viajes; unos 200 de los 330 µs.
- Arreglo: orden nueva "leer por nombre": busca, lee hasta lo que quepa en la ventana y no deja asa abierta. Devuelve
  el tamaño total, para que el cliente siga con LEER si no cupo. Documentarla en `servicios.md`.
- Ganancia estimada: leer 4 KB hacia 7.000 a 9.000/s.
- Esfuerzo: pequeño.

**A4. Ventana de 1 MB, múltiplo del registro** [C]
- Dónde: `archivos.rs:46-51` (`TRAMO`, `VENTANA_DISCO`, `VENTANA`, todas de 64 KB); `transaction.rs`, escritura de
  registros (hacia la línea 2296).
- Qué pasa al leer: 8 MB son 128 viajes.
- Qué pasa al escribir: el registro de RedoxFS es de 128 KB. El primer trozo de 64 KB crea un registro de 64 KB; el
  segundo lo relee, lo copia a uno de 128 KB y suelta el viejo. Cada 128 KB cuestan 192 KB escritos y 192 KB pasados
  por lz4.
- Arreglo: ventana negociada en la sesión, 1 MB por defecto (la respuesta de SESION ya lleva el tamaño). El cliente
  escribe en trozos múltiplos de 128 KB. Cuidado con la memoria: 8 clientes son 8 MB; darla al primer uso grande, no
  al abrir sesión.
- Ganancia estimada: leer 8 MB ya leídos hacia 700 a 800 MB/s; escribir 8 MB hacia 100 MB/s y un tercio menos de
  escrituras en la SD.
- Esfuerzo: pequeño.

**A5. Leer sin copia intermedia y sin clonar el asignador** [C en el clon, L en las copias]
- Dónde: `transaction.rs:110-131` (`Transaction::new` hace `fs.allocator.clone()` en cada transacción, también las
  de lectura); `archivos.rs:174-186` (`Cache::leer` copia el bloque); lectura de registros en `transaction.rs`
  (hacia 514-594 y 2182).
- Qué pasa: cada LEER clona el asignador (crece con la fragmentación), copia de la caché a un búfer de registro
  puesto a ceros y de ahí a la ventana.
- Arreglo: (a) transacción de solo lectura que no clone nada; (b) cuando el registro no está comprimido, copiar de
  la caché a la ventana directamente.
- Ganancia estimada: con A4, leer 8 MB ya leídos hacia 1.000 MB/s. [S]
- Esfuerzo: medio. Antes, perfilar con el `PERFIL` del servicio para saber cuánto pesa cada parte.

**A6. No comprimir lo que no comprime** [L]
- Dónde: `transaction.rs`, `lz4_flex::compress_into` en cada registro de más de un bloque (hacia 2325).
- Arreglo: probar con los primeros 4 KB; si no bajan, saltar el registro. Quitar la relectura del registro viejo para
  comparar cuando el fichero es nuevo.
- Esfuerzo: pequeño.

### Fase 7: lo grande

**A7. Caché de páginas compartida** [S en la ganancia]
- Hecho (2026-11-11): trozos de 512 KB prestados con PRESTAR; ver la bitácora en `REVISION_LOOP.md`.
- Qué pasa hoy: aunque el dato esté en memoria, cada lectura es un viaje y una copia.
- Arreglo: el servicio presta al cliente páginas de la caché, en solo lectura, como llaves de memoria. El cliente lee
  sin viaje mientras el fichero no cambie. Hace falta: caché por fichero y no por bloque de disco (lo descomprimido),
  invalidación al escribir, y que el kernel permita proyectar más de 4 MB o por trozos.
- Es el paso que iguala la lectura de Linux. También quita casi todo el coste de leer para varios clientes a la vez.
- Esfuerzo: grande. Depende de la fase 8 (memoria) si se quiere bien hecho; una primera versión con ventanas de 4 MB
  por fichero abierto ya vale.

**A8. Transacciones en grupo y asignador sin copia entera** [C en el código, S en el peso]
- Dónde: `transaction.rs:110-131` (clon), `sync_allocator` (hacia 255-355), `allocator.rs:134` y `:167` (recorridos
  en línea).
- Qué pasa: cada orden de metadatos es una transacción completa. Con el disco fragmentado, cada una copia más.
- Arreglo: una transacción abierta entre peticiones que se cierra al guardar; asignador con diario de cambios en vez
  de clon; libres en una estructura ordenada.
- Ganancia estimada: borrar con guardado al final, de 1.932 hacia 3.000 a 4.000/s. Evita que se degrade con el disco
  lleno.
- Esfuerzo: medio o grande.

**A9. Un disco dañado o malicioso no tumba el servicio** [L]
- Dónde: `transaction.rs:62, 71, 181, 380, 770, 948, 1026, 1171, 1195, 1241, 1281, 1598` (`assert`, `expect`,
  `unwrap`); `:586-588` (largo sacado del disco usado como índice); `:1901-1919` (recorrido sin detectar ciclos);
  `block.rs:33, 39, 44, 77`; `filesystem.rs:314-327` (un `count` del disco decide cuánta memoria se pide);
  `header.rs:150-151`. El pánico de un servicio es `salir(101)` (`usuario/src/lib.rs:305-309`).
- Qué pasa: una tarjeta ajena o rota mata el servicio para todos los programas. La suma seahash detecta daños, no
  manipulación.
- Arreglo: cada `assert` sobre datos del disco pasa a `Err(EIO)`; validar largos y niveles al leer; tope de
  profundidad en los recorridos. Añadir una prueba que abra imágenes con bytes al azar cambiados.
- Esfuerzo: medio. Es de seguridad y debe ir pronto si se va a montar discos de fuera.

**A10. Comprobación y reparación al arrancar** [L]
- Dónde: `archivos.rs:1273` (`FileSystem::open(disco, None, Some(0), true)` solo hace `cleanup`).
- Qué pasa: COMPROBAR (16) solo informa. `archivos/redoxfs/src/repair.rs` existe y ninguna orden lo llama. Descartar
  una instantánea puede no devolver su espacio hasta reparar (`docs/ARCHIVOS_LOOP.md:256`).
- Arreglo: marca de "cerrado limpio" en la cabecera; si falta, comprobación rápida sin datos y reparación del
  asignador. Una orden REPARAR. COMPROBAR por trozos, para no parar a los demás clientes.
- Esfuerzo: medio.

**A11. El comprobador y la cuarentena** [C]
- El aviso "FS8 comprobación: MAL6 blocks lost" que salió en una tanda fue falso. Con el guardado diferido, los
  bloques soltados esperan en cuarentena y `check` los contaba como perdidos. Ya está arreglado: COMPROBAR guarda
  antes (`archivos.rs:1044-1048`). No se encontró ningún camino que pierda bloques de verdad.
- Queda frágil: quien llame a `check` con la cuarentena llena volverá a verlo. Arreglo: pasar `fs.quarantine` al
  comprobador como "libre pendiente". Esfuerzo: pequeño.

**A12. `sync_allocator` cambia memoria antes de confirmar** [S]
- Dónde: `transaction.rs:325-336` (cambia `fs.fresh` y llama a `disk.forget`), `:2366` ("TODO: deallocate new_addr on
  failure?").
- Qué pasa: si la escritura posterior falla, un bloque aún referenciado puede quedar fuera de la caché y sin escribir.
- Arreglo: aplazar esos cambios hasta que la transacción confirme. Probar con errores de disco inyectados (el mismo
  disco de pruebas de A2).
- Esfuerzo: pequeño.

**A13. Permisos por cliente** [L] — decisión de Fran (S6)
- Dónde: `archivos.rs:725-727` (el asa 0 es la raíz del volumen 0 para cualquiera); COMPROBAR y las órdenes de
  volúmenes las puede pedir cualquier cliente y la contraseña viaja en el mensaje; sin cupo de espacio por cliente. En
  `nvme.rs`, quien tiene llave ve el disco entero.
- Lo que está bien: BUSCAR rechaza partes vacías, no sigue enlaces y no se vio escape de ruta.
- Arreglo: `inicio` da a cada programa una raíz acotada a un cajón, con derechos (leer, escribir, administrar). Las
  órdenes de volumen, instantáneas y comprobación piden "administrar". Cupo por cliente.
- Esfuerzo: medio.

**A14. Lo que falta para la Pi con tarjeta SD (FS11)** [C en `sd.rs` y la fecha, L en el resto]
- `usuario/src/bin/sd.rs:1-2`: solo lee dos sectores, por consulta, sin DMA y sin escribir. No ofrece el protocolo
  de disco. Hace falta un controlador de verdad: bloques múltiples, escritura, DMA, 50 MHz o más, el mismo protocolo
  que `nvme.rs`.
- Sin descarte (TRIM): ni en `nvme.rs` ni en el `trait Disk` (`archivos/redoxfs/src/disk/mod.rs`; su
  `impl Disk for Box<dyn Disk>` no reenvía `forget`).
- Sin tabla de particiones: el sistema empieza en un desplazamiento fijo de 1 MB (`INICIO`).
- Sin fechas: `archivos.rs:530`, `AHORA = 0`. Hace falta un servicio de hora (reloj de la placa o de red).
- Sin lectura anticipada: cada lectura fría pide 64 KB y espera.
- Lo demás de FS11 (alinear a 4 MB, juntar escrituras) sigue pendiente y necesita la placa.
- Esfuerzo: grande.

**A15. Un solo hilo y disco con cola de uno** [L]
- Dónde: bucle de `principal` en `archivos.rs`; `nvme.rs:49-50` ("las órdenes van de una en una"); `nvme.rs:65`
  (`MAX_CLIENTES = 4`, nunca se liberan); `archivos.rs:52` (`MAX_CLIENTES = 8`).
- Qué pasa: una lectura fría, un guardado o un COMPROBAR paran a todos. No se nota en los bancos, que tienen un solo
  cliente; se notará con varios programas.
- Arreglo corto: varias órdenes NVMe en vuelo, COMPROBAR por trozos, subir los topes y liberar huecos al morir un
  cliente. Arreglo de fondo: A7, y hilos en el servicio cuando existan (fase 8).

**A16. NVMe: tras un plazo vencido queda inservible** [C]
- Dónde: `nvme.rs`, `Nvme::orden` (`:153-195`): al vencer el plazo devuelve `Err(u32::MAX)` sin avanzar `c.cabeza`.
- Qué pasa: la terminada que llega tarde se toma por la de la orden siguiente, el identificador no casa y desde ahí
  todo da error. Un tropiezo del disco es definitivo.
- Arreglo: al vencer, vaciar las terminadas hasta casar el identificador, o reiniciar el controlador. Devolver error
  al cliente y seguir.
- Esfuerzo: medio. Prueba: provocar el plazo (parar el disco de QEMU desde el monitor).

**A17. Menores** [L]
- `archivos.rs:198-204`: al llenarse, la caché ordena todos los usos (hasta 10.240). Mejor reloj o lista.
- `archivos.rs:315-331`: si falta un bloque del rango, lo relee entero.
- La caché es un `BTreeMap` con una caja de 4 KB por bloque: una petición de montón por bloque. Mejor trozos grandes.
- LISTAR recorre todos los hijos en cada página: cuadrático con miles de entradas.
- Los nombres recordados se vacían enteros al llenarse o con un RENOMBRAR.
- Al acortar un fichero, el último registro conserva su tamaño.
- Formato: la cabecera es un anillo de 256 y abrir las lee todas; un fichero de 4.096 bytes justos ya no cabe en el
  nodo (3.968 bytes dentro) y necesita un registro aparte.

## 5. Kernel: el camino de un mensaje

Ficheros: `kernel/src/llamadas.rs`, `kernel/src/objetos.rs`, `kernel/src/base/planificador.rs`,
`kernel/src/base/excepciones.rs`, `kernel/src/base/usuario.rs`, `kernel/src/temporizadores.rs`,
`kernel/src/interrupciones.rs`.

### Fase 3: sin cambiar la ABI

**K1. Resolver el hilo y el proceso una vez por llamada** [C]
- Dónde: `excepciones.rs`, `excepcion` (al salir: `replanificar_si_toca`, `debe_morir`, `ttbr0_actual`);
  `planificador.rs`, `aqui` (`:1150`), `proceso_actual_arc` (`:1132`); `base/usuario.rs`, `directa` (`:46-65`);
  `llamadas.rs:112` (`planificador::recoger()` en cada llamada).
- Qué pasa: cada una de esas funciones enmascara, lee el número de núcleo y toma el cerrojo `NUCLEOS[n]`. Cada copia
  sube y baja un `Arc` y toma el cerrojo del espacio por página. Una ida y vuelta son 4 llamadas y decenas de tomas.
- Arreglo: puntero al hilo actual por núcleo (en `TPIDR_EL1`), `matar` como atómico leído sin cerrojo, `ttbr0` leído
  del hilo. Dentro de una llamada el proceso se toma prestado: el hilo ya tiene su referencia. `recoger()` solo si un
  atómico dice que hay muertos.
- Esfuerzo: medio. El ahorro hay que medirlo.

**K2. Tabla de llaves sin recorridos** [C]
- Dónde: `objetos.rs`, `TablaLlaves::huecos` (`:548-551`) e `instalar` (`:525-538`); `llamadas.rs:312`
  (`t.huecos()` en cada `canal_leer`, traiga o no llaves); `ver` clona la `Llave` entera.
- Qué pasa: `huecos` recorre hasta 1.024 entradas en cada lectura.
- Arreglo: contador de libres y lista de huecos; no mirar huecos si el mensaje no trae llaves; `ver` sin clonar.
- Esfuerzo: pequeño.

**K3. Saltar el camino de llaves cuando no viajan llaves** [C]
- Dónde: `llamadas.rs`, `canal_escribir` (segundo `proc_.llaves.tomar()`, `leer_usuario` de 0 bytes, dos `Vec`) y
  `canal_leer` (`escribir_usuario` de una tabla vacía, dos `Vec`).
- Arreglo: con `n_llaves == 0`, nada de eso.
- Esfuerzo: pequeño.

**K4. Mensajes sin pedir al montón global** [C]
- Dónde: `llamadas.rs:75-81` (`leer_usuario`: `try_reserve_exact`, `resize(n, 0)` y luego la copia encima);
  `base/monton.rs:26` (un cerrojo para todos los núcleos); `interrupciones.rs:243` y `temporizadores.rs:357`
  (`to_le_bytes().to_vec()` por cada aviso).
- Qué pasa: cada mensaje pide un bloque, lo pone a cero, copia encima y lo libera en otro núcleo.
- Arreglo recomendado: una caché por núcleo de bloques de mensaje de tamaño fijo (64 y 256 bytes), sin cerrojo
  global, y copiar sin poner a cero antes. Los avisos del kernel (4 bytes) usan la de 64.
- Por qué no meter los datos dentro de la cola, como anillo fijo: serían unos 17 KB por canal aunque esté vacío.
  `COSTE_CANAL` ya lo cobra, pero hoy un proceso ocupa 122 KB de verdad y esa ventaja frente a Linux se perdería.
- Esfuerzo: medio.

**K5. PAN de hardware donde lo haya** [S]
- Dónde: `excepciones.rs` (`activar_vacio` al entrar, `activar` al salir); `base/espacio.rs:444-446`.
- Qué pasa: dos cambios de TTBR0 por llamada. En la Pi 3 y la Pi 4 (Cortex-A53 y A72) no hay PAN: no se puede
  quitar. En la Pi 5 y en QEMU con una CPU moderna sí.
- Arreglo: mirar `ID_AA64MMFR1_EL1` al arrancar; con PAN, no tocar TTBR0 y usar el bit PAN. Las copias ya van por el
  mapa directo (`directa`), así que el cambio es pequeño.
- **No optimizar más el cambio de TTBR0 para el emulador.** Primero B5.
- Esfuerzo: medio.

### Fase 4: ABI nueva

**K6. Llamada de ida y vuelta con paso directo** [C en lo que falta, S en la ganancia] — decisión de Fran
- Dónde: `docs/referencia/kernel.md` (llamadas 4 y 5); `objetos.rs`, `Canal::escribir` (`:432-449`, despierta a todos
  los que esperan); `planificador.rs`, `despertar` y el robo del ocioso.
- Qué pasa hoy: una petición son 4 entradas al kernel y dos despertares con aviso a otro núcleo. El núcleo ocioso
  puede robar el hilo recién despertado. Es M6 de la bitácora, sin hacer.
- Diseño propuesto (números nuevos; 4 y 5 no se tocan):
  - 32, "canal_llamar": escribe por una llave y espera la respuesta por otra, sin volver a EL0 entre medias.
    Argumentos: llave de envío, datos, largo, llave de respuesta, búfer, cabida, tabla de llaves recibidas y su
    cabida. No manda llaves (para eso siguen 4 y 5). Devuelve lo mismo que `canal_leer`.
  - 33, "canal_responder_esperar": lo mismo para el servidor; contesta a un cliente y se queda leyendo su canal de
    servicio.
  - Paso directo: si el destino está bloqueado leyendo ese canal, se copia del búfer de quien manda al búfer de quien
    espera (los dos por el mapa directo, sin montón) y se le cede la CPU en este mismo núcleo, con lo que quede del
    turno. Si no está esperando, o el cerrojo de su espacio no se consigue a la primera (`intentar`), se va por la
    cola de siempre. Así no hay abrazo mortal entre dos procesos que se escriben a la vez.
  - Despertar a uno, no a todos.
  - Se respetan: marca de la llave, cupos, `E_LIMITE`, la comprobación de muerte (C39), la regla de no ciclos.
- Con esto también se arregla la inversión de prioridad (M7): el servidor corre con el tiempo de quien le llama.
- Ganancia: de 4 a 2 entradas por ida y vuelta y sin avisos entre núcleos. En QEMU, donde cada entrada cuesta unos
  15 µs, sería bajar de 62 hacia 30. En placa real no se sabe: por eso va después de B5.
- El servicio de archivos y `usuario/src/ficheros.rs` pasan a usarlas; también `kit/` y el camino de Dash (lo
  segundo, por la sesión de Dash).
- Esfuerzo: grande. Documentar en `kernel.md`. Prueba: los bancos en Rust, C y Dash, y una prueba de dos procesos
  que se llaman a la vez.

**K7. Plazo al leer y aviso de muerte por canal** [C en lo que falta]
- Qué pasa hoy: `canal_leer` no tiene plazo y no hay forma de enterarse por un canal de que un proceso murió. Un
  plazo se hace creando un temporizador hacia el mismo canal (así lo hace `archivos`): dos llamadas más por espera.
- Diseño propuesto, sin objeto nuevo: los servicios ya atienden a muchos por un canal con llaves marcadas, y
  temporizadores e interrupciones ya avisan a un canal. Basta con:
  - un plazo en 32, 33 y en una lectura nueva con plazo (devuelve un error propio al vencer);
  - "proceso_avisar": cuando el proceso de esa llave acabe, escribe un aviso con marca en un canal dado, como hace
    un temporizador.
- No hace falta una espera sobre varios canales: sería un objeto más en el kernel para lo mismo.
- De paso: que `canal_leer` no devuelva `E_CERRADO` cuando lo que pasó es falta de memoria o que mataron al hilo
  (`llamadas.rs:297-300`).
- Esfuerzo: medio. Es condición de la fase 5.

### Fase 1: planificador y temporizadores, arreglos pequeños

**K8. La admisión de la clase de plazo no cuenta a los dormidos** [C]
- Dónde: `planificador.rs`, `crear_plazo` (`:418-424`) y `plazo_actual` (`:447-470`).
- Qué pasa: `usado` suma solo los hilos de `nc.listos`. Un hilo de plazo dormido o bloqueado no cuenta. Varios
  procesos pueden pedir casi el 80 % cada uno mientras los otros duermen; juntos pasan del 100 % y dejan sin CPU a
  los demás. Hoy lo puede pedir cualquier proceso.
- Arreglo: un total "prometido" por núcleo, que sube al entrar en plazo y baja al salir o morir. Y pedirlo con una
  llave (S3).
- Esfuerzo: pequeño.

**K9. Con 64 hilos dormidos en un núcleo, dormir no duerme** [C]
- Dónde: `planificador.rs:308` (`dormidos.reserve(64)`) y `:648-653` (si no cabe: `despierta = 0` y el hilo sigue
  corriendo).
- Qué pasa: un bucle activo silencioso, alcanzable desde programas.
- Arreglo: lista enlazada dentro de `Hilo` (sin reserva), o devolver un error.
- Esfuerzo: pequeño.

**K10. Un temporizador llega tarde a propósito** [C]
- Dónde: `temporizadores.rs`, `Temporizador::nuevo` (`holgura = intervalo / 16`, hasta 1 ms) y `programar` (el reloj
  se pone para `tarde`, no para `vence`).
- Qué pasa: uno de 1 ms llega 62 µs tarde aunque no haya otro con el que juntarse. Va dentro del p99 medido.
- Arreglo: programar para `vence`, y usar la holgura solo si otro temporizador cae dentro de la ventana.
- Esfuerzo: pequeño.

**K11. Una interrupción con la cola llena queda apagada para siempre** [C]
- Dónde: `interrupciones.rs:244`, `let _ = canal.escribir(lado, m);`.
- Qué pasa: el aviso se tira, la interrupción sigue enmascarada y nadie llamará a `interrupcion_terminar`. Los
  controladores de USB comparten ese canal con un temporizador de 10 ms: uno lento lo puede llenar. Que pase en la
  práctica es sospecha; el código es así.
- Arreglo: un bit "pendiente" por interrupción y reentrega cuando el lector vacíe la cola.
- Esfuerzo: pequeño o medio.

**K12. Aviso perdido en el repartidor (solo GICv3)** [C]
- Dónde: `interrupciones.rs`, `repartidor` (`:188-200`): mira `PENDIENTES` antes de tomar `REPARTIDOR`.
- Qué pasa: `terminar()` desde otro núcleo puede marcar y despertar en ese hueco. La interrupción espera al siguiente
  despertar. No afecta a la Pi 4.
- Arreglo: volver a mirar con el cerrojo tomado, como ya se hace con los muertos.
- Esfuerzo: pequeño.

### Otros del kernel

**K13. Temporizadores y repartidor solo en el núcleo 0** [S]
- Dónde: `temporizadores.rs:254`, `interrupciones.rs:182`, `planificador.rs:823`.
- Qué pasa: todo temporizador pasa por el núcleo 0, con un presupuesto de 300 µs por milisegundo. Una llamada larga
  en ese núcleo los retrasa a todos.
- Arreglo: cola de temporizadores por núcleo, entregada donde espera el hilo. Antes, medir cuánto gasta el
  repartidor por pasada.
- Esfuerzo: medio.

**K14. Huecos de 10 a 21 ms con más hilos que núcleos** [C en la medida]
- Qué pasa: con 5 a 8 procesos calculando, el hueco máximo es de uno o dos tics de 10 ms. Para 60 imágenes por
  segundo con los 4 núcleos al 100 % (meta de S0) es demasiado si quien espera es el compositor.
- Arreglo: lo que S2 ya pide y falta: pesos por foco (M8) y que el compositor y el sonido vayan en clase de plazo con
  la admisión arreglada (K8). Con K6, el servidor hereda el turno del cliente.
- Esfuerzo: medio.

**K15. Menores** [L]
- `planificador.rs:828` y `base/cpu.rs:22-27`: el reloj se reprograma en cada decisión, con `isb`. Recordar lo
  programado y escribir solo si cambia. [S]
- `planificador.rs:160-205`: `NUCLEOS`, `CARGA`, `REPLANIFICAR` y los contadores son arrays seguidos que escriben
  todos los núcleos. Una estructura por núcleo alineada a 64 bytes. No se nota en QEMU; en placa, sí.
- `base/espacio.rs:431`: `ic ialluis` (vacía la caché de instrucciones de los 4 núcleos) por cada página de código al
  crear un proceso. Una vez al final. [C]
- `base/monton.rs:26` y `base/memoria.rs:62`: un cerrojo global cada uno. Cachés por núcleo cuando K4 esté hecho y
  medido.
- `interrupciones.rs:8-9`: el comentario dice que el montón no enmascara las interrupciones; ya las enmascara.

## 6. Seguridad

Las cinco auditorías de `docs/seguridad/` están cerradas en su mayor parte. Siguen abiertos o a medias: llamadas
largas (primera auditoría, punto 14), DMA sin IOMMU (punto 4, mitigado), temporizadores dentro de un grupo (fase 4,
M9), `buzon.rs` (B19) y la cuarentena de DMA (B20). De S16 quedan sin cumplir las propiedades 1 (sin pánicos), 2
(todo cobrado), 5 (azar) y 7 (trabajo acotado), y las fases 5 (fuzzing, Kani, Miri) y 6 (azar de la placa, arranque
verificado).

**S1. El kernel no debe caer cuando se acaba su montón** [C en los sitios, S en lo alcanzable]
- Dónde: `planificador.rs` (`Box::new` del hilo y del estado de coma flotante; `push` en `listos`, `muertos` e
  `hilos` que crecen pasada la reserva); `objetos.rs`, `Canal::devolver`; `interrupciones.rs:243` y
  `temporizadores.rs:357` (`to_vec`). `docs/referencia/kernel.md:377-378` lo admite: el grupo raíz no tiene cupo.
- Qué pasa: son reservas que no pueden fallar; sin memoria, pánico. La de coma flotante la dispara un programa con
  solo usarla. Los 32 MB reservados al kernel lo hacen difícil, no imposible. Además se cobra de menos: la tabla de
  llaves llena puede ocupar unos 80 KB y se cobran 64.
- Arreglo: `try_reserve` y `Box::try_new` en todo lo que se alcanza desde un programa; estado de coma flotante
  reservado al crear el hilo; colas enlazadas dentro de `Hilo`; cobrar el tamaño real; cupo también para la raíz.
- Esfuerzo: medio.

**S2. Llamadas largas con las interrupciones cerradas** [C en el código, S en el efecto]
- Dónde: toda llamada corre con las interrupciones cerradas (solo se abren en el ocioso y al arrancar un hilo).
  `memoria_crear` admite 4 MB: `base/memoria.rs`, `Memoria::nueva` toma hasta 1.024 páginas y `pagina_usuario` pone a
  cero cada una con el cerrojo global de páginas tomado. `procesos.rs` copia hasta 4 MB al crear un proceso.
  `its.rs`, `encender`, hasta un millón de vueltas con cerrojo.
- Qué pasa: en el núcleo 0 eso retrasa temporizadores e interrupciones. Es candidato a los máximos de 686 a
  1.721 µs del temporizador con carga.
- Arreglo: poner a cero fuera del cerrojo; puntos de corte cada N páginas (abrir interrupciones y mirar si toca
  ceder); una reserva de páginas ya a cero que rellena el ocioso.
- Esfuerzo: medio. Medir el máximo del temporizador antes y después.

**S3. La clase de plazo, con llave** [C] — decisión de Fran
- Ver K8. S2 dice que hace falta permiso; el código dice "falta decidir quién puede pedirlo".

**S4. Llaves de aparato con todos los derechos** [L]
- Dónde: `usuario/src/bin/inicio.rs:84` y `:111` (`llave_duplicar(…, 7, …)`); `base/espacio.rs:394` y
  `base/placa.rs:32-34` (`es_periferico` admite toda la ventana, con el GIC dentro).
- Qué pasa: un controlador puede pasar a otro su llave de registros.
- Arreglo: entregar sin el derecho de transferir, derechos mínimos por tipo de llave, y excluir de `es_periferico`
  el GIC y lo que usa el kernel.
- Esfuerzo: pequeño.

**S5. Azar adivinable** [L]
- Dónde: `kernel/src/azar.rs` (splitmix64 sobre el contador), `kernel/src/elf.rs`. Lo admite `kernel.md`.
- Qué pasa: solo se mueven los ejecutables PIE, unos 21 bits; la pila va fija. Sin dirección al azar del kernel.
- Arreglo: el generador de la placa (RNG200 en la Pi 4, `virtio-rng` en QEMU), página de guarda bajo la pila de
  usuario (comprobar si existe), rechazar ejecutables de dirección fija fuera de las pruebas.
- Esfuerzo: medio.

**S6. Endurecer el arranque** [L]
- Dónde: `base/arranque.s:22-47`.
- Qué pasa: SCTLR sin WXN; no se fijan `CPTR_EL2` ni `MDCR_EL2` (en placa real depende del firmware); entrando en EL1
  se conserva el SCTLR del firmware. Montón y pilas reciben páginas sin poner a cero (`monton.rs:42`, `mapa.rs:242`).
- Arreglo: fijar esos registros siempre y poner a cero lo que se da al montón.
- Esfuerzo: pequeño cada uno. Lo de la placa real es sospecha hasta probar.

**S7. Un USB hostil o roto tumba el servicio USB** [C en `usb.rs`, L en el resto]
- Dónde: `usuario/src/bin/usb.rs:367` (`mps` sale del aparato) y `:298` (`largo.div_ceil(mps)`): un aparato que
  diga 0 da división por cero. `usuario/src/bin/xhci.rs:512` (índice del controlador sin acotar).
  `usuario/src/bin/virtio_sonido.rs:401-403` (índice fuera de los 64 KB de DMA).
- Qué pasa: se pierde teclado y ratón.
- Arreglo: acotar cada índice y aceptar solo 8, 16, 32 o 64 como tamaño de paquete.
- Esfuerzo: pequeño.

**S8. El teclado se lo lleva cualquier ventana nueva** [L]
- Dónde: `wayland/src/escena.rs:463-472`; `usuario/src/bin/sonido.rs` (el mensaje TIC no comprueba que venga de
  "virtio-sonido"; SALIDA sí).
- Qué pasa: un programa abre una ventana y recibe lo que se teclea. Cualquier cliente del sonido adelanta el reloj
  de la mezcla.
- Arreglo: el foco solo cambia por una acción del usuario; comprobar la marca en TIC.
- Esfuerzo: pequeño.

**S9. Sin IOMMU en la Pi 4** [L]
- Un controlador comprometido puede hacer que su aparato escriba en memoria física dentro de su ventana de DMA. Está
  documentado y mitigado (primer GB reservado). No tiene arreglo en esta placa; sí conviene la cuarentena de DMA
  (B20 de la auditoría).

Ver también A9 (disco malicioso) y A13 (permisos de ficheros), que son de seguridad.

## 7. Estabilidad y servicios

### Fase 1: arreglos pequeños

**E1. Un mensaje vacío tumba el compositor** [C]
- Dónde: `usuario/src/bin/compositor.rs:344-350`. El búfer es fijo y no se limpia: con `n == 0`, `buf.first()` es el
  byte del mensaje anterior. Si era un trozo de datos, hace `&buf[1..n]` con `n = 0`: pánico.
- Qué pasa: cualquier cliente conectado tira el escritorio. El kernel admite mensajes de 0 bytes.
- Arreglo: al principio del bucle, con `n == 0`, cerrar las llaves y seguir.

**E2. Fuga de llaves y giro al 100 %** [C en el giro, L en la fuga]
- Dónde: `compositor.rs:312-314` y `usuario/src/bin/sonido.rs:125-127` (`if n < 0 { continue; }` sin dormir);
  `wayland/src/canal.rs:108-114` (las llaves de los trozos se acumulan sin tope); en `sonido.rs`, los brazos TIC,
  VOLUMEN y CERRAR no cierran las llaves que llegan.
- Qué pasa: 128 mensajes con 8 llaves llenan la tabla de 1.024. El siguiente da `E_LIMITE`, vuelve a la cola y el
  servicio gira para siempre. El de sonido va en clase de plazo: se come 3 ms de cada 5,3.
- Arreglo: cerrar siempre las llaves que sobren, tope de llaves por cliente y dormir un tic en error, como ya hacen
  `buzon.rs` y `archivos.rs`.

**E3. Wayland sin topes por cliente** [L]
- Dónde: `wayland/src/servidor.rs:229-238` (objetos), `wayland/src/escena.rs:432-439` (daños y avisos de cuadro).
- Qué pasa: un cliente agota los 16 MB del compositor y este aborta.
- Arreglo: tope de objetos, de daños (al pasarse, un solo rectángulo) y de avisos por superficie; error de protocolo
  al superarlos.

**E4. El evento de tecla se pierde con el canal lleno** [L]
- Dónde: `usuario/src/hid.rs:20-24`. La tecla queda pulsada.
- Arreglo: guardar el último estado y reenviarlo, o mandar el estado completo en vez de cambios.

**E5. Esperas sin plazo y bucles activos** [L]
- Sin plazo: `usuario/src/bin/gpu.rs:111-116`, `virtio_sonido.rs:388`, `usuario/src/sonido_cliente.rs:89-93`,
  `archivos.rs:299-311` (`pedir_por`).
- Girando: `usb.rs:286-291`, `usuario/src/bin/consola.rs:18-26`, `buzon.rs:129-144`, `nvme.rs:271`,
  `virtio_sonido.rs:243-249`.
- Arreglo: una espera con plazo común (con K7 es una sola llamada) y un estado "aparato caído".

### Fase 5: lo grande

**E6. Nadie vigila ni relanza los servicios** [C]
- Dónde: `usuario/src/bin/inicio.rs:159-171`: solo espera a los programas sin llaves y luego se queda leyendo un
  canal propio para siempre.
- Qué pasa: si cae el compositor, el sonido, el disco o el servicio de archivos, no vuelven hasta reiniciar.
  `inicio` conserva su lado de los canales, así que los clientes ni siquiera ven el cierre.
- Arreglo: `inicio` como supervisor. Con K7 recibe el aviso de muerte de cada servicio, lo relanza con las mismas
  llaves (tope de reintentos y espera creciente) y avisa a quien dependía de él. Los servicios deben reusar la
  entrada de un cliente que vuelve con la misma marca (`archivos.rs` ya lo hace; `nvme.rs` no: A15).
- Orden de relanzado por dependencias: disco, archivos, compositor, sonido.
- Esfuerzo: grande. Prueba: matar cada servicio en `make arranque-virt` y comprobar que vuelve.

**E7. Una biblioteca común para los servicios** [L]
- Dónde: el mismo bucle (leer con marca, directorio de 24 bytes, cerrar llaves) está escrito cinco veces:
  `compositor.rs:307-325`, `sonido.rs:121-139`, `buzon.rs:181-211`, `nvme.rs:466-493`, `archivos.rs:1291-1352`. Cada
  copia tiene fallos distintos: de ahí E1, E2 y A16.
- Arreglo: un módulo `servicio` en `usuario/src/` con el bucle común: cierra las llaves que sobran, duerme en error,
  lleva la tabla de clientes por marca, suelta al que no lee y recibe los avisos de muerte. Un módulo de protocolo
  por servicio, compartido con su cliente (hoy las constantes están duplicadas). Llaves con nombre en el paquete de
  arranque, en vez de adivinarlas por posición con `existe(k)`.
- Esfuerzo: grande, pero quita código. Hacerlo antes de E6.

**E8. Compositor: componer a ritmo de cuadro y no frenar por un cliente** [L]
- Dónde: `compositor.rs:244-270` y `wayland/src/escena.rs:226-256` (píxel a píxel); `compositor.rs:386-390` (compone
  tras cada mensaje); `:212-233` (`mandar` reintenta 50 veces de 1 ms); `:51-75` (crea y cierra un canal con el buzón
  en cada cuadro).
- Qué pasa: un cliente que no lee frena a todos 50 ms por evento.
- Arreglo: copiar por filas, componer con un temporizador de cuadro y solo si hay daño, soltar al cliente con la
  cola llena, canal fijo con el buzón.
- Esfuerzo: medio. Falta una medida de cuadros por segundo con carga: es la meta de S0 y hoy no se mide.

**E9. Sonido: coma flotante por software y cuatro copias** [L en las copias, S en el coste]
- Dónde: `usuario/src/bin/sonido.rs`, `sonido/src/lib.rs:101-125` y `:257-292`. El destino de compilación es sin
  coma flotante de hardware; el remuestreo va en `f64` por muestra.
- Qué pasa: el propio código dice que un período de dos flujos cuesta 1 a 2 ms en QEMU, con un período de 5,3 ms.
- Arreglo: coma fija (Q15 o Q31) y mezclar directo del anillo del cliente a la ranura de salida.
- Esfuerzo: medio. Medir en placa.

**E10. USB** [L]
- Sin conexión en caliente (`xhci.rs:386-388`), sondeo cada 10 ms (`usb.rs:555-570`), y `usb.rs:546` supone que el
  aparato 1 es un concentrador.

## 8. Capacidades que faltan (fase 8)

No son fallos; son límites del diseño de hoy que frenan lo demás. Están en `docs/referencia/kernel.md`.

- **Un proceso, un hilo.** No hay llamada para crear hilos (el kernel ya guarda una lista de hilos por proceso). Sin
  hilos, el servicio de archivos no puede atender a uno mientras espera al disco por otro. Es lo primero de esta
  fase.
- **Memoria.** `memoria_crear` da hasta 4 MB; no hay carga por demanda, ni copia al escribir, ni ficheros
  proyectados; cada proceso copia su código. S3 lo pide todo. Lo que más rinde: páginas de código compartidas entre
  procesos del mismo programa y proyección de ficheros (con A7).
- **Colas de 16 mensajes de 256 bytes.** Bien para órdenes; todo dato grande ya va por memoria compartida. No
  cambiarlo.
- **Identidad y permisos (S6).** No hay usuario ni grupo por proceso, ni permisos por aplicación, ni firmas. Solo la
  marca de 64 bits de la llave. A13 es la primera pieza.
- **Cifrado por usuario (FS10)** y **cuidado de la SD (FS11)**: decididos y sin hacer.

## 9. Lo que está bien y no hay que tocar

- Kernel pequeño (3,2 MB en memoria) y 122 KB por proceso: 4 a 10 veces menos que Linux. Cualquier cambio que suba
  esto debe justificarse con una medida.
- `deny(unsafe_code)` fuera de `base`; W^X en kernel y programas; pilas con guarda; copias de usuario que miran
  permisos y no leen dos veces; cerrojos que enmascaran interrupciones; colas y tablas con tope y cobradas; un
  mensaje que no se entrega vuelve intacto; revocación con marca.
- En archivos: sumas por bloque con copia al escribir, anillo de cabeceras, un solo vaciado para guardados pequeños,
  comprobador completo, crear con datos y `stat` en un viaje. Listar ya gana a Linux.
- En servicios: el buzón, el bucle de `archivos`, la validación de `wl_shm`, las marcas que pone `inicio`.

## 10. Lo que no se revisó

`kit/`, `audiodevice/`, `amigados/`, `usuario/src/graficos/`, `usuario/src/bin/imagen.rs`, `kernel/src/inicio.rs`,
`kernel/src/pruebas.rs`, `uefi/`, y de RedoxFS: `tree.rs`, `htree.rs`, `dir.rs`, `seal.rs`, `snapshot.rs`,
`volume.rs`, `key.rs`, `adiantum.rs`. No se opina del cifrado ni de si la prueba de corte de luz cubre el guardado
diferido. PCIe en placa real sigue sin probar. Nada se midió en hardware.
