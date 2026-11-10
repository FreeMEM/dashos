# Revisión: implantar mejoras y usuarios (plan y bitácora)

Encargo del usuario (2026-11-10): implantar `docs/REVISION_ARQUITECTURA.md` y `docs/diseño/S6b-usuarios-y-permisos.md`,
con todas sus decisiones aprobadas, "como mejor creas", en loop. Papel de esta sesión: implementa (no lee Linux ni
desensamblados).

Worktree `.worktrees/revision`, rama `revision` (sale de `multitarea`). Commits locales: nada hacia fuera sin permiso.

## Cómo se trabaja

- Cada paso: medir antes si toca rendimiento (`make -C kernel medir`, `tools/medir.py`), cambiar, `make -C kernel
  prueba` y `make -C kernel arranque-virt`, apuntar aquí y commit local.
- Cada arreglo de seguridad o estabilidad entra con su prueba hostil, que falla antes y pasa después.
- Emulador bajo `flock /run/user/1000/dash-emu-red.lock`. Parar solo procesos propios por PID.
- Al 90 % de `~/.claude-ftapias/state/uso-7d-ftapias.txt` se para.
- Quien cambie una llamada o una orden pone al día `docs/referencia/kernel.md` o `servicios.md` en el mismo commit.

## Orden

Primero lo que hace falta para probar (cliente hostil), luego lo pequeño y seguro, luego permisos en archivos (U1, que
no depende de nada), luego lo grande. Las medidas de rendimiento esperan a tener el banco fiable (B1 a B3).

| Paso | Qué | Puntos | Estado |
|---|---|---|---|
| R1 | Cliente hostil en las pruebas del arranque | B6 | hecho |
| R2 | Servicios: mensajes vacíos, fugas de llaves, giros, topes de Wayland, tecla perdida | E1 a E5 | hecho |
| R3 | Kernel: dormidos sin tope, temporizador puntual, interrupción con la cola llena, aviso perdido | K9 a K12, K15 | hecho (lo de placa, en la Pi) |
| R4 | Llaves de aparato con derechos mínimos, USB hostil, foco del teclado, TIC del sonido | S4, S7, S8 | hecho (falta sacar el GIC de la ventana) |
| R5 | Clase de plazo con llave y admisión que cuenta a los dormidos | S3, K8 | hecho |
| R6 | Archivos con ficha: dueño, grupo y modo, raíces, ceder, administrar | U1, A13 | hecho (ceder asas, con R17) |
| R7 | Banco fiable | B1 a B4 | hecho |
| R8 | Archivos, lo rápido: cerrar sin guardar, menos viajes | A1 a A6 | hecho (A5, la mitad: falta copiar sin búfer intermedio) |
| R9 | Aviso de muerte y plazo al leer | K7 | hecho |
| R10 | Azar de la placa y arranque endurecido | S5, S6 | azar hecho (falta RNG200 y arranque) |
| R11 | Kernel sin pánicos por memoria y llamadas largas troceadas | S1, S2 | en parte (ver bitácora) |
| R12 | Supervisión de servicios y biblioteca común | E6 a E9 | E6, E9 y casi todo E8 (E7, pendiente) |
| R13 | Discos ajenos en un proceso lector aparte | S6b regla 4 | hecho para ADF (FAT y ext4, cuando existan) |
| R14 | `sesiones`: cuentas, fichas, lanzar desde disco, procesos | U2 | primera parte (fichas, procesos, matar) |
| R15 | Entrada, bloqueo y volumen cifrado por usuario | U3 | pendiente |
| R16 | Permisos por aplicación | U4 | pendiente |
| R17 | `guardian` y `selector` | U5 | pendiente |
| R18 | Camino caliente del kernel; archivos, lo grande | K1 a K5, A7 a A14 | pendiente |
| R19 | `Prefs` de usuarios, registro de seguridad, PID en el kernel | U6 | pendiente |

K6 (ida y vuelta con paso directo) espera a medir en la Pi real (B5).

## Bitácora

### 2026-11-10

- Decisiones del usuario apuntadas en S6, S6b, S4b, RESUMEN y en el informe. La sonda de medidas de `multitarea` se
  quitó (parche guardado fuera del repo).
- R1: `usuario/src/bin/hostil.rs`, en el arranque de `virt`. Ataca a archivos, sonido y compositor con mensajes vacíos y
  160 mensajes de 8 llaves que no usan, y comprueba con un plazo de 3 s que cada uno sigue contestando.
- R2: compositor (E1: mensaje vacío; E2: tope de 32 llaves esperando por conexión, cerrar las llaves de un trozo que
  no se mete y de ADIÓS, esperar 1 ms si no puede leer), sonido (E2: cerrar las llaves de TIC, VOLUMEN y CERRAR;
  esperar si no puede leer), Wayland (E3: 1.024 objetos y 16 pools por cliente, 32 daños por superficie juntados en
  uno), USB (E4: los eventos que no caben esperan, en orden, y se reintentan). `make prueba` pasa también las pruebas
  de PC de `wayland/` (25) y `sonido/` (8). E5 queda para R9: necesita la lectura con plazo de K7.
- R3: K9 (dormidos en lista enlazada dentro del hilo: sin tope), K10 (el reloj va al último vencimiento que cabe en
  la holgura del más urgente: un temporizador solo no llega tarde), K11 (una interrupción con la cola llena se
  reintenta cada milisegundo), K12 (mirar pendientes otra vez con el cerrojo), K15 (la caché de instrucciones, una
  vez por carga; comentario del montón). Quedan de K15 lo que hay que medir en placa.
- R4: S4 con un bit nuevo, "una vez" (8): la llave llega sin el derecho de transferir. `inicio` lo pone en las llaves
  de aparato; `malo` lo prueba (pasos 32 y 33). S7: tamaño de paquete USB solo 8 a 64, ranura del xHCI acotada,
  cabeza de virtio-sonido acotada. S8: TIC solo del controlador; una ventana nueva no se lleva el teclado si en los
  últimos 2 s se pulsó una tecla en la de otro programa (prueba de PC). Pendiente de S4: sacar el GIC de la ventana de
  periféricos (valor bajo; toca `base`).
- R5: K8 (lo prometido a la clase de plazo se cuenta por núcleo, con los que duermen) y S3 (la llamada 28 pide un
  permiso de plazo, objeto nuevo que `inicio` da al sonido y a virtio-sonido; `malo` paso 34).
- R6: archivos con fichas. `inicio` manda el alta (orden 100, sin marca) con usuario, grupo, derechos y cajón raíz
  (`FICHAS`, provisional). Permisos de Unix sin paso libre para el 0, asa 0 = raíz del cliente, derecho de
  administrar para volúmenes, instantáneas y comprobar, contraseña por la ventana, 64 clientes. El disco de pruebas
  sale con dueños fijos (`archivos/herramientas`, `duenos`) y un `C/Privado` 0700. `hostil` es el 1001 encerrado en
  `C`. Pendiente de U1: ceder asas (va con el selector, R17) y grupos adicionales.
- A1 y A2 (R8, adelantados): cerrar ya no espera al disco; un guardado que falla se dice en el siguiente cerrar,
  vaciar o guardar de quien tenía cambios, y el disco pasa a solo lectura hasta una comprobación limpia. Falta una
  prueba que fuerce el fallo. A3: orden 28, leer por nombre (buscar, leer y cerrar en un viaje), probada en
  `ficheros` y en `hostil`; sin medir aún.
- R10, primera parte: el azar del kernel es ChaCha20 con la llave que cambia tras cada uso, sembrado con
  `/chosen/rng-seed` y `kaslr-seed` del árbol (los ponen el firmware de la Pi y QEMU), que se borran del original.
  Llamada 34, `azar`, que dice si es de calidad. Prueba del bloque con el vector de la RFC 8439 en el arranque. Con
  él, archivos hace volúmenes cifrados (orden 23, a = 1 o 2); sin azar de calidad, -14. Falta el RNG200 de la Pi 4.
- Pruebas en QEMU: esperando al emulador (lo tiene la batería de la 0.9.4-alfa de Tachyon, hasta las 18:00). Se
  prueban en copias aparte (`.worktrees/r1-antes` sin arreglos, `.worktrees/r-prueba` con R2 a R5) y después el árbol
  de trabajo.
- Primeras pruebas (18:00 a 19:00). Sin arreglos (`r1-antes`), el hostil tumba el compositor (pánico en
  `compositor.rs:350`, E1) y deja sin contestar al sonido (E2). Con R2 a R5, `make prueba` pasa (un C14 con 1.762 µs
  de desviación fue ruido: en la vuelta siguiente pasó). Lo que salió:
  - Las llaves de un programa van en el orden en que cada canal se nombra por primera vez en `PROGRAMAS`, no en el de
    su lista: el hostil tenía archivos y sonido cambiados.
  - Archivos suelta al cliente que no lee sus respuestas (cola llena): el hostil abre otra sesión tras inundar.
  - Un volumen cifrado cerrado ya no se lista ni vacío: su raíz no se lee y no hay permisos que mirar. La raíz de un
    volumen nace con su dueño (`create_volume_owned` en el fork de RedoxFS, `REDOXFS_DUENO` en `redoxfs-volume`).
  - Con S8, Ajustes sale mientras se teclea en otra ventana y no se lleva el teclado: la prueba de virt le da el
    teclado con un clic en (300, 300) cuando la salida dice que está en pantalla, y entonces maximiza.
  - QEMU `virt` da 40 bytes de semilla (`rng-seed` y `kaslr-seed`): el azar es de calidad y los volúmenes cifrados se
    hacen en DashOS.
- El usuario pide (2026-11-10) medir según se cambia y poner al día el panel de medidas con cada medida.

- Con todo lo de arriba más G2 (mapa de teclado), `make prueba` y `make arranque-virt` pasan enteros (2026-11-10,
  tras la batería de Tachyon): hostil sin tumbar nada, FS10, A3, archivos con fichas, sonido sin huecos, captura del
  compositor, disco leído en Linux. La prueba de UEFI ya es determinista (espera a Ajustes en pantalla, clic y
  Super+Arriba/Abajo).
- Gráficos G3 a G6 (detalle en `GRAFICOS_LOOP.md`): subsuperficies, capas del escritorio, bloqueo de sesión y
  dmabuf, probados en el PC con swaybg, swaylock y alacritty (directo y por el puente).
- R7, banco fiable: el orquestador se fija en su núcleo; eco y ping en los dos siguientes (el núcleo del orquestador
  cuenta como ocupado y el kernel no pondría ahí un proceso nuevo); una carga fijada por núcleo que calcula hasta que
  acaba la medida (bandera en memoria compartida). El ping hace 500 rondas de calentamiento, N seguidas (la media de
  siempre, comparable con Linux) y N con el reloj en cada una (mediana, p99 y máximo, por histograma; leer el reloj
  en cada ronda cuesta). 7 arranques; el JSON guarda mediana, mínimo y máximo, y `comparar` dice "dentro del ruido".
  QEMU fijado con `taskset` a 6 núcleos libres del PC, uno por núcleo físico, y la carga del PC en el JSON (sin
  fijar, la ida y vuelta sale un 12-16 % peor).
- R9, K7: llamada 35, leer con plazo (`E_PLAZO`, -9); llamada 36, aviso de fin de proceso por canal (16 bytes:
  clase y código, con la marca de la llave; hasta 16 por proceso). `canal_leer` ya no dice `E_CERRADO` sin memoria.
  Pruebas: `malo` pasos 35 y 36 (plazo, plazo enorme, rechazos) y el lanzador pide el aviso de sus 7 pruebas antes y
  después de acabar (7 de 7).
- R8: A4, ventana de hasta 1 MB pedida en SESION (el hostil pide una enorme y le dan 1 MB). A6, RedoxFS prueba 4 KB
  antes de comprimir un registro. Medido (7 arranques): escribir 8 MB por trozos de 1 MB 180 MB/s (78 con 64 KB),
  leer 705 MB/s (405).
- R11: S2, las páginas de usuario se ponen a cero fuera del cerrojo del repartidor (y se quitó un segundo borrado al
  crear tablas y páginas). S1: los avisos de interrupción, temporizador y fin ya no hacen pánico sin montón. La parte
  de reservar el hilo antes de dar las llaves se quitó: cualquier reserva de más en `procesos::lanzar` hacía la ida
  y vuelta un 10 % más lenta en QEMU (seis medidas para acotarlo: no es la FPU, ni la línea de caché del hilo, ni el
  asignador). Pendiente: entender por qué la colocación de esos objetos pesa tanto.
- R12: E6, `inicio` vigila los servicios (los que leen algún canal): guarda una copia de sus llaves, pide su aviso de
  fin y los relanza con el mismo canal tras 100, 200 y 400 ms; a la cuarta, los deja. Prueba en virt con `caida`,
  que se cae a propósito y ve en su canal el mensaje que le deja `inicio`. Falta reenviar el directorio y las altas
  a un servicio relanzado y avisar a quien dependía de él. E8, en parte: el compositor espera como mucho 5 ms a un
  cliente con la cola llena y lo suelta; también a los que no leen sus eventos de entrada o de cuadro.
- Medidas frente a `f9adec2` (7 y 3 arranques, `medir-antes.json`): sin carga, igual (dentro del ruido); con 4
  cargas, la ida y vuelta 133 a 89 µs (-33 %) y las esperas tarde 825 a 50 (-94 %); temporizador con carga, p99 884
  a 538 µs. Ficheros: crear 4K +7 %, escribir 8 MB +11 %, datos +4 %. `make prueba` y `make arranque-virt` pasan.
- El banco de ficheros medía mal "crear_4k_sync" desde A1: decía que cerrar ya era como fsync, y cerrar ya no espera
  al disco. Ahora vacía tras cada fichero, como `fs.c` en Linux: 345 ficheros/s (Linux 442; antes de A1, 375). Sin
  vaciar, 1.006 (Linux sin fsync, 3.107).
- E8, componer: por cada fila dañada, tramos entre los bordes de las ventanas; cada tramo empieza por la ventana opaca
  (XRGB) más alta que lo cubre, o por el fondo: lo de debajo no se pinta. Un tramo ARGB del todo opaco se copia; si
  no, se mezcla sin saltos (rojo y azul a la vez en un entero, exacto: se comparó con la fórmula vieja). Medido en el
  PC (`medir_componer`, 800 x 600 entera con tres ventanas): 311 a 225 µs por cuadro (-28 %); la ventana a medias
  sola, 182 a 107 µs. Dividir entre 255 o copiar fila a fila sin más no ganaba nada (el compilador ya lo hacía).
  Falta de E8: componer con un temporizador de cuadro y el canal fijo con el buzón.
- E6, lo que faltaba: al relanzar un servicio, `inicio` le vuelve a dar, tras el aviso de relanzado, la entrada del
  directorio de cada cliente de sus canales y, al de archivos, sus altas (sin montón: un mapa de bits de clientes por
  canal, y se rehacen desde el paquete de arranque). Prueba: `caida` tiene un cliente (`caida-cliente`) y cada
  relanzado lo ve en su directorio. Falta avisar a los clientes de que su servicio volvió (sus sesiones se pierden).
- R13: los ADF ya se leen en un proceso aparte (`imagen`, con solo el canal de archivos, y `inicio` lo relanza si
  cae). Prueba hostil nueva (`amigados/tests/danadas.rs`, en `make prueba`): 20.000 copias del disquete de pruebas
  con bytes cambiados al azar (sobre todo arranque, raíz y alrededores) y las sumas de los bloques rehechas, como
  haría quien lo prepara a mala idea; se abren, se listan, se leen y se cuentan los libres. Ninguna tumba el lector;
  la más lenta, 13 µs; 6.527 llegan a listarse. Los lectores de FAT y ext4 tendrán que ir igual y con la misma prueba.
- R14, primera parte: el servicio `sesiones` (solo en `virt` de momento). `inicio` le da de alta, sin marca, cada
  programa que lanza después de él: marca (el PID), usuario, grupo, nombre y una copia de la llave de su grupo de
  recursos. Los programas piden por su canal la lista de procesos (por páginas de 7: un mensaje son 256 bytes) y
  matar; solo se mata lo del mismo usuario, que sale de la marca de quien pide. Usuarios provisionales en `inicio`
  (`usuario_de`): los de `FICHAS`, `presa` el 1001 y los servicios el 1 (nadie corre como 0). Prueba hostil: `hostil`
  (1001) ve 20 procesos, no mata a `ficheros` (1000), sí a `presa` (1001), y un PID que no existe da -1. Como su canal
  es el primero que se nombra, en `hostil` las llaves se corren una (sesiones 1, ventanas 2, archivos 3, sonido 4).
  Falta de U2: lanzar desde disco, la tabla de nombres, repartir las llaves (hoy, `inicio`), la baja al morir un
  proceso (con K7) y el grupo nuevo de un servicio relanzado.
- U2, la baja: `inicio` pide para cada programa el aviso de fin (K7) hacia `sesiones`, con la marca de su ficha y el
  bit alto puesto (ningún programa puede escribir con esa marca), y `sesiones` quita la ficha al llegar. En virt salen
  las bajas de los que acaban (disco, ficheros, hola, tono-a con clase 2, que falla a propósito...). La prueba del
  hostil ahora intenta matar al compositor (usuario 1), porque `ficheros` puede haber acabado ya, y comprueba que
  `caida-cliente`, que acabó solo, ya no está en la lista.
- U2: un servicio relanzado vuelve a tener ficha en `sesiones`, con su grupo nuevo, y su baja pedida otra vez. En
  virt, `caida` sale tres veces como "vuelve, con su grupo nuevo".
- A5, primera mitad: una transacción de RedoxFS ya no copia el asignador al empezar; lo copia la primera vez que
  reserva o libera (`allocator: Option`). En el PC (`perfil`, 285 trozos libres), una transacción vacía pasa de
  1,2 µs a 60 ns; escribir, igual. En QEMU (banco de ficheros, 7 arranques), todo dentro del ruido: con el disco de
  pruebas recién hecho, el asignador es pequeño. Se notará con discos fragmentados. Falta la otra mitad: copiar de la
  caché a la ventana sin pasar por un registro puesto a ceros.
- E9: la mezcla del sonido en coma fija. Muestras en enteros con 1,0 = 32.768, volúmenes con 1,0 = 65.536 y el
  remuestreo con 32 bits de fracción; solo convertir F32 y la API de volumen usan coma flotante. Medido en el servicio
  (lo que tarda cada período con sonido, en `arranque-virt`): de 320 µs de media y 1.024 de peor a 20 µs y 285
  (-94 %). Con 4 cargas (`sonido-carga`), 16 µs. Las pruebas de sonido (`arranque-virt`, `sonido-carga`,
  `sonido-paula`) siguen sin huecos ni chasquidos; las 8 de PC del mezclador, adaptadas (comparan lo mismo pasando a
  -1..1). Falta de E9: mezclar directo del anillo del cliente a la ranura de salida.
- R18, K2 y K3: la tabla de llaves lleva la cuenta de sus huecos y desde dónde buscar (antes, `huecos` recorría hasta
  1.024 entradas en cada lectura); un mensaje sin llaves no toma la tabla ni al escribir ni al leer. Medido (7
  arranques, frente a r19): la ida y vuelta baja del 3 al 8 % en todas las variantes (entre núcleos, 64,1 a 59,7 µs;
  con carga, 90,3 a 83,9), casi todo dentro de la dispersión pero en el mismo sentido. `malo` (llaves repetidas,
  tabla llena, canales cerrados) y el hostil pasan.
