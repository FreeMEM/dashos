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
| R4 | Llaves de aparato con derechos mínimos, USB hostil, foco del teclado, TIC del sonido | S4, S7, S8 | hecho |
| R5 | Clase de plazo con llave y admisión que cuenta a los dormidos | S3, K8 | hecho |
| R6 | Archivos con ficha: dueño, grupo y modo, raíces, ceder, administrar | U1, A13 | hecho |
| R7 | Banco fiable | B1 a B4 | hecho |
| R8 | Archivos, lo rápido: cerrar sin guardar, menos viajes | A1 a A6 | hecho |
| R9 | Aviso de muerte y plazo al leer | K7 | hecho |
| R10 | Azar de la placa y arranque endurecido | S5, S6 | hecho (falta RNG200 y probar en placa) |
| R11 | Kernel sin pánicos por memoria y llamadas largas troceadas | S1, S2 | hecho |
| R12 | Supervisión de servicios y biblioteca común | E6 a E9 | hecho (E8: cola de salida por cliente, medida de cuadros y sin daños repetidos) |
| R13 | Discos ajenos en un proceso lector aparte | S6b regla 4 | hecho para ADF (FAT y ext4, cuando existan) |
| R14 | `sesiones`: cuentas, fichas, lanzar desde disco, procesos | U2 | fichas, procesos, matar, lanzar desde el disco y tabla de nombres |
| R15 | Entrada, bloqueo y volumen cifrado por usuario | U3 | hecho en pruebas, también por tiempo (falta bloquear con una tecla) |
| R16 | Permisos por aplicación | U4 | repartir y revocar, preguntar al usuario (en una capa encima de todo) y guardar lo concedido |
| R17 | `guardian` y `selector` | U5 | ceder asas, ventana del selector (elige el usuario, con cajones y doble clic), acciones de administrador |
| R18 | Camino caliente del kernel; archivos, lo grande | K1 a K5, A7 a A14 | K1, K2 y K3; K4 probado sin ganancia; A7 (caché de páginas prestada); A8 (el asignador; la transacción abierta, medida, no compensa), A9, A10, A11, A12, A13 y A16 (NVMe); de A14, fechas, TRIM, lectura anticipada y tabla de particiones |
| R19 | `Prefs` de usuarios, registro de seguridad, PID en el kernel | U6 | registro de seguridad (en memoria y en el disco), PID en el kernel y Permisos en el centro de control (S10c, C1) |

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
- K4, probado y deshecho: un montón por núcleo (un cerrojo por núcleo para los bloques pequeños, con lo soltado en
  la lista de quien lo suelta) no gana nada en QEMU: la ida y vuelta queda igual o algo peor (de +1 a +15 %, dentro
  del ruido). Cada mensaje se pide en un núcleo y se suelta en otro, así que los bloques van de una lista a otra; y
  en QEMU el cerrojo único apenas se disputa. Queda para medirlo en la Pi (B5), con una caché de bloques de mensaje
  de tamaño fijo como dice el informe y no como montón general. Copiar sin poner a cero antes (la otra parte de K4)
  no se hizo: son 256 bytes como mucho.
- K1, un trozo: `recoger()` (en cada llamada al kernel) ya no toma el cerrojo del núcleo si un indicador por núcleo
  dice que no hay hilos muertos. Medido: dentro del ruido (-2 a +5 %); se deja porque quita un cerrojo por llamada,
  que en placa con cuatro núcleos sí se disputa. Lo grande de K1 (hilo actual en TPIDR_EL1, proceso prestado sin
  subir y bajar el Arc) sigue pendiente.
- E7, primer trozo: `dashos::protocolo::archivos` (órdenes, estados, tipos, cabecera y ventanas) y
  `dashos::protocolo::sesiones`, sacados de los servicios. Se quitaron 143 copias exactas de 6 programas (archivos,
  ficheros, hostil, medir-fs, imagen, adf-prueba); las que no coinciden se quedan, y como un nombre local tapa al del
  módulo, nada cambia sin querer. Pasan `arranque-virt`, `prueba` y el banco de ficheros (`fs-bien`). Falta lo grande
  de E7: el bucle común de los servicios (leer con marca, cerrar llaves sobrantes, dormir en error, clientes por
  marca, soltar al que no lee).
- E7, el bucle común: `dashos::servicio::recibir` (leer con marca; si el canal se cerró, `Cerrado`; si no se pudo
  leer, dormir 1 ms y `Nada`) y `cerrar_llaves`, usados por buzón, imagen, sesiones, compositor y sonido. `recibir`
  exige búferes del máximo del kernel (256 bytes, 8 llaves). **Fallo hostil encontrado**: `sesiones` leía con 64
  bytes y 4 llaves; un mensaje con 8 llaves (la inundación nueva del hostil) no se podía leer, se quedaba el primero
  de la cola y el servicio ya no atendía a nadie. Además giraba sin dormir al fallar la lectura (E2). Con el tipo del
  búfer en `recibir`, ningún servicio que lo use puede volver a caer. El hostil ahora inunda también `sesiones` y
  luego comprueba que sigue contestando. Faltan archivos y NVMe (hacen trabajo entre leer y mirar el error).
- R15 (U3), primera parte: `sesiones` es cliente del servicio de archivos (ficha con el derecho de administrar
  volúmenes, raíz entera; no lee lo de nadie) y atiende ENTRAR y SALIR: abre o cierra el volumen cifrado del usuario,
  con la contraseña por la ventana (A13) y borrada de su memoria tras usarla (regla 9). Solo los pide `guardian` (en
  las pruebas, `entrada`, que hace de él). Disco de pruebas: volumen "Carla" (1001, "clave de Carla"). `sesiones`
  va ahora tras archivos y `inicio` da de alta también lo lanzado antes; en el hostil, las llaves pasan a ventanas 1,
  archivos 2, sesiones 3, sonido 4. Pruebas: `entrada` entra con contraseña mala (-12), con la buena (0) y sale (0);
  el hostil intenta entrar como Carla con la contraseña buena y recibe -12. `ficheros` ve ahora dos volúmenes.
  Falta de U3: la pantalla de `guardian` y el bloqueo de la sesión (compositor G5).
- U3, volver de un bloqueo: orden 29 de archivos, COMPROBAR_CLAVE (RedoxFS `check_volume_password`: deriva la llave
  y la tira, sin abrir ni tocar el volumen), y VOLVER en `sesiones`. Tras una contraseña mala, `sesiones` tarda
  300 ms en contestar. `entrada` comprueba volver con una mala (-12) y con la buena (0) con el volumen abierto.
- R16 (U4), primera parte: `sesiones` reparte llaves de servicios. `inicio` le da una copia sin marca de los
  servicios que reparte (`REPARTE`, hoy solo `eco`, un servicio de prueba que contesta lo que le llega); con la
  orden SERVICIO, si la ficha del programa tiene el permiso (`PERMISOS`, provisional), `sesiones` hace una copia
  marcada con su número, que viaja una vez (UNA_VEZ: llega sin el derecho de transferir, regla 2), la hace revocable,
  escribe su entrada en el directorio del servicio y se la da. DEJAR la revoca. Prueba: `pide` (con permiso) usa
  `eco`, no puede regalar la llave (-3), renuncia y la llave ya no sirve (-2); el hostil la pide sin permiso: -2.
  Falta de U4: los permisos declarados en el paquete (S8), preguntar al usuario (`guardian`) y guardarlo.
- U1 (lo que faltaba) y R17 (U5), primera parte: orden 30 de archivos, CEDER. Un cliente con el derecho de repartir
  (nuevo, `D_REPARTIR`) da a otro cliente (por su marca) una asa nueva para el nodo de una suya, con derechos de leer
  y escribir que no pueden pasar de los que él tiene. Con una asa cedida se mira lo cedido, no los permisos del
  fichero. Un `selector` de prueba (usuario 1000, con el derecho) cede a quien se lo pide la lectura de
  `C/Privado/nota.txt` (0600, del 1000). Prueba hostil: el hostil (1001, encerrado en `C`) lee con esa asa, pero no
  escribe en ella, no la cede (no tiene el derecho) y sigue sin abrir el fichero por su nombre. En el hostil, el
  canal "selector" es la llave 5. Falta de U5: la ventana del selector y `guardian` con sus acciones de administrador.
- R19 (U6), el registro de seguridad: `sesiones` apunta cada negativa (matar a otro usuario, la llave de un servicio
  sin permiso, entrar o volver con contraseña mala o sin ser `guardian`) y cada proceso que muere por un fallo, con
  quién y qué, en un anillo de 64 en memoria; REGISTRO lo da por páginas, solo a `guardian`. Prueba: `entrada` lee
  6 entradas y encuentra los tres intentos del hostil; el hostil pide el registro y se le niega. Falta guardarlo en
  el disco (cuando `sesiones` tenga su cajón), Prefs de usuarios, PID en el kernel y cambio rápido de usuario.
- U2, lanzar desde el disco: LANZAR en `sesiones` (solo `guardian`; luego también el escritorio y el Shell). Busca el
  fichero por el servicio de archivos, lo copia por la ventana a un objeto de memoria y lo crea con `proceso_crear_en`
  en un grupo propio de 4 MB (sale del cupo de `sesiones`, de 16), con ficha: número desde 0x10000, el usuario de
  quien lo pide y el nombre del fichero. El disco de pruebas trae `Programas/hola` (el `hola` que se construye).
  Prueba: `entrada` lanza `Programas/hola` (sale su "hola desde Rust") y un texto (-1); el hostil no puede (-2).
  Falta la tabla de nombres.
- U2: lo que lanza `sesiones` nace con una llave, su canal con `sesiones` marcado con su número (S6b: "nace con una
  llave"), y tiene baja con su aviso de fin. Para eso `inicio` da a `sesiones` su copia propia del canal sin marca.
  El disco trae también `Programas/pide`: lanzado desde ahí, pide la llave de `eco` por el canal con que nació y
  repite la prueba de U4 con su número nuevo (65537); llegan las bajas de `hola` (65536) y de `pide`.
- R10, segunda parte (S6, arranque endurecido): al bajar de EL2, `CPTR_EL2` queda solo con sus bits RES1 y
  `MDCR_EL2` a cero salvo HPMN (nada atrapa hacia EL2, donde no hay quien atienda); si el firmware entra ya en EL1,
  SCTLR_EL1 se pone al valor conocido, no se hereda. Con el mapa del kernel puesto, cada núcleo enciende
  SCTLR_EL1.WXN: la CPU no ejecuta nada que se pueda escribir, en EL1 ni en EL0 (antes no se puede: los bloques del
  arranque se escriben y ejecutan). El montón pone a cero cada página nueva y cada bloque grande, y las pilas nuevas
  salen a cero; `alloc_zeroed` no repite el borrado de lo grande. Prueba hostil (S6 en `pruebas.rs`): ensucia dos
  pilas y un bloque de 64 KB, los suelta y pide otra vez; vuelven las mismas páginas (2 de 2 y el mismo bloque) y
  sin un byte sucio; WXN puesto. Sin medir: el coste es un borrado de 4 KB por página nueva del montón y uno por
  página de pila nueva, fuera de los caminos que se miden. Falta probarlo en la placa (con el firmware real).
- R11, S1 (segunda parte): crear un hilo o un proceso ya no hace pánico sin montón. El hilo se reserva con
  `Box::try_new` (feature `allocator_ext`); las colas `listos` y `muertos` de cada núcleo tienen siempre sitio para
  todos los hilos que existen (contador `HILOS`, `asegurar_colas` al crear; no encogen), así el planificador nunca
  pide memoria con las IRQ enmascaradas; la lista de hilos del proceso se reserva antes. `procesos::lanzar` usa
  `Arc::try_new` y, si el hilo no se puede crear, suelta el proceso y devuelve `SinMemoria` (las llaves ya estaban en
  su tabla y se cierran con él). El estado de coma flotante, si no hay memoria para él, mata al hilo en su vuelta a
  EL0 en vez de tumbar el kernel. Prueba hostil (S1 en `pruebas.rs`, con un interruptor de fallos del montón por
  núcleo que solo existe en las pruebas): lanza `hijo` haciendo fallar la reserva 1, luego la 2..., hasta que ya
  no falla ninguna. La primera vuelta encontró un pánico (en la propia prueba); ahora: 11 reservas, las 11 dan
  `SinMemoria`, 0 páginas perdidas. Sin probar: la falta de memoria para la coma flotante (no se puede provocar a
  voluntad desde la prueba). Falta de S1: cobrar el tamaño real de la tabla de llaves y cupo para la raíz.
- Medidas de S6 y S1 (7 arranques cada una, todas seguidas y con la misma carga del PC; `046cd57` medido otra vez
  como base): S6 sale igual (-1 a -3 %, ruido). La primera versión de S1 ponía la ida y vuelta un 4 a 7 % más
  lenta (mediana), con el cálculo igual. Causa: `asegurar_colas` hacía crecer `listos` y `muertos` en las clases
  pequeñas del montón, y eso cambiaba dónde caían los demás objetos del kernel. Con las colas de una página (512
  hilos) desde el arranque, fuera de esas clases: -1 a -5 % frente a la base, dentro del ruido; el kernel ocupa
  32 KB más (3.456 a 3.488 KB). Seguramente es lo mismo que hizo un 10 % más lenta la reserva del hilo antes de
  dar las llaves (R11, primera parte): el banco es muy sensible a la colocación de los objetos calientes en QEMU.
  Lo pendiente es saber por qué; en la Pi real puede no pasar.
- R11, S1 (lo que faltaba): la tabla de llaves crece al doble pero nunca pasa de `MAX_TABLA + 1` entradas
  (`crecer`, con `try_reserve_exact`): llena ocupa 41 KB, no los 80 de antes (crecía a 2.048). `COSTE_PROCESO` ya
  no es un número a ojo: la tabla llena (`TABLA_LLENA`, por el tamaño de `Option<Llave>`), la pila del kernel y
  8 KB para el hilo, la coma flotante, el proceso y el redondeo del montón: 65.576 bytes. Prueba (S1 en
  `pruebas.rs`): llena una tabla hasta que no admite más (1.024 llaves, 41.000 bytes) y suma lo demás con los
  tamaños de verdad, redondeados a las clases del montón: 59.320, por debajo de lo cobrado. El cupo de la raíz ya
  existía (la memoria libre para procesos menos la reserva del kernel, desde la fase 3): la revisión lo daba por
  pendiente. Sin medir: el crecimiento es el mismo que antes hasta 1.024 llaves.
- Fallo encontrado al preparar S2: un proceso que dormía 500 µs en los núcleos 2 o 3 despertaba a los 10 ms (mediana
  9.570 µs; en los núcleos 0 y 1, 30). Al quedarse ocioso un núcleo mientras otro tiene carga, `vigilar_al_dormir`
  reprogramaba su reloj a un tic entero y pisaba el despertar más próximo (un dormido, un plazo). Ahora programa el
  menor de los dos. Después: mediana de 27 a 32 µs y máximo por debajo de 600 en los cuatro núcleos. El banco no lo
  veía porque sus hijos van en los núcleos 0 y 1.
- R11, S2 (puntos de corte): `planificador::punto_de_corte` abre un instante las interrupciones dentro de una llamada
  larga, donde no hay cerrojos tomados ni una copia de usuario a medias; el temporizador puede entrar y expulsar al
  hilo. La imagen de pruebas cuenta los cerrojos tomados por núcleo (`cerrojo::tomados`; en la normal no cuesta
  nada) y un corte con alguno tomado no abre y se apunta. Cortes: `memoria_crear` cada 64 páginas
  (`Memoria::nueva_en_llamada`) y la copia del binario en `proceso_crear` cada 256 KB (`leer_en_llamada`). Prueba
  hostil (S2 en `pruebas.rs`, `hijo` con las órdenes 11 y 12): un proceso duerme 100 veces 500 µs en el núcleo 3
  mientras otro, en el mismo núcleo, crea y suelta 60 objetos de 4 MB. Retraso al despertar (mediana, p90, p99,
  máximo): antes 1.123, 1.176, 1.801 y 1.848 µs; después 90, 125, 253 y 631 (solo: 28, 49, 154 y 614). 976 cortes,
  ninguno con cerrojo. Después, `elf::cargar_cortando` (`procesos::lanzar_en_llamada`, solo desde `proceso_crear`):
  un corte cada 64 páginas de segmentos y pila. Sin prueba propia: los programas de las pruebas cargan menos de 64
  páginas, así que ese corte no llega a abrirse en ellas. Queda `its.rs`, `encender` (solo al arrancar, en virt).
  Banco (7 arranques, frente a S1): todo dentro del ruido. `make prueba` y `make arranque-virt` pasan.
- U2, la tabla de nombres: orden 10 de `sesiones`, NOMBRES. Contesta la tabla de quien pide (por su marca): `SYS:` y
  `Programas:` para todos, `Home:` en el volumen de su usuario si tiene cuenta (`CUENTAS` provisional: 1000 Ana,
  1001 Carla) y `PROGDIR:` con el cajón de donde se lanzó, si salió del disco (la ficha lo guarda al LANZAR). Cada
  entrada: nombre, volumen (vacío es el disco) y subcamino. La biblioteca (`protocolo::sesiones::Nombres`) la lee y
  resuelve "Nombre:resto" sin distinguir mayúsculas, y no resuelve lo que intenta salir del subcamino ("/" al
  principio, que en el Amiga es subir, o ".."). No es frontera de seguridad: lo que se alcanza lo deciden las raíces
  del servicio de archivos. Prueba: `pide` lanzado del disco ve `PROGDIR:` = `Programas` y resuelve
  `PROGDIR:pide`; lanzado por `inicio`, sin él; sin `Home:` (usuario 1); `PROGDIR:/hola`, `SYS:a/../../b` y un
  camino sin dos puntos no se resuelven. `hostil` (1001) recibe su tabla: `Home:` es Carla, no Ana. Falta: las
  raíces 0 a 15 en archivos (hoy una por cliente) para que cada nombre sea una raíz cedida, `RAM:T`, y las
  asignaciones con capas (S4b).
- U1, capa 2 de S6b (las raíces): en `archivos`, las asas 1 a 15 son raíces con tope (R, o R y W), además de la 0 de
  la ficha; las de buscar y crear empiezan en 16. CEDER con un hueco en el segundo byte de b pone la asa como raíz
  de otro (hueco libre o `SIN_SITIO`), y la marca `YO` deja a quien cede darse una raíz más estrecha. Por una raíz
  manda el permiso del fichero y además el tope; lo abierto desde ella lo hereda (`topes`); escribir, crear,
  borrar, renombrar (también en el destino), recortar, comentar y proteger piden la W del tope. Las raíces no se
  cierran, se sueltan con el cliente y al volver a una instantánea de su volumen. Prueba: `selector` (dueño del
  disco) se da `C` como raíz 1 de solo leer: por la asa de `C` de su raíz 0 crea y borra; por la 1 no crea, no borra,
  no escribe lo que abre desde ella (tope heredado) y no la cierra; una segunda cesión al mismo hueco, -6. `hostil`
  recibe `C` como raíz 2 de solo leer: lista, no crea ni la cierra, un hueco vacío da -5, no se da raíces (sin el
  derecho de repartir) y otra cesión no pisa la suya. Sin medir: una búsqueda de más por petición en un mapa casi
  siempre vacío. Falta que `sesiones` ceda las raíces de cada nombre de la tabla al lanzar (el programa aún no tiene
  sesión de archivos cuando nace).
- U2, nacer con raíces: `inicio` da a `sesiones` una copia sin marca del canal de archivos (su llave 5). Al LANZAR,
  `sesiones` manda antes el alta del programa (orden 100, sin marca): su usuario, la raíz 0 en el cajón de donde
  sale (`PROGDIR:`), la 1 el disco para leer (`SYS:`, `Programas:`) y la 2 su volumen para leer y escribir (`Home:`)
  si tiene cuenta; y le da su llave de archivos marcada con su número (la segunda con que nace). `archivos` lee las
  raíces de más del alta (hueco, tope, volumen y camino; la que no encuentra, la dice y sigue) y se las pone al
  abrir la sesión. La tabla de nombres dice ahora en qué raíz está cada nombre (`SIN_RAIZ` para lo que no lanzó
  `sesiones`: por volumen y subcamino desde su raíz 0). Prueba: `pide` lanzado del disco abre su sesión con esa
  llave y por la tabla encuentra `PROGDIR:pide`, `Programas:hola` y `SYS:LEEME.txt`; por `SYS:` no crea (-12) ni
  la cierra (-5). `hostil` manda un alta suya con el disco entero para escribir en el hueco 5: no le da nada (un
  alta solo vale sin marca). Falta: `RAM:T` y raíces de cada paquete (S8).
- U2/U3, lanzar como el usuario que entró: `sesiones` apunta quién tiene la sesión abierta (ENTRAR y SALIR de
  `guardian`), y LANZAR lleva "como quién" (bytes 8 a 11): solo `guardian` y solo como un usuario que entró. No se
  sale mientras siga vivo un programa de ese usuario lanzado por `sesiones` (-13). Orden nueva de archivos, BAJA
  (101, sin marca): con la baja de lo que lanzó, `sesiones` le dice a archivos que suelte su sesión, sus asas y
  raíces y su alta (sin ella, su raíz `Home:` dejaba el volumen ocupado para siempre). Prueba: `entrada` entra como
  Carla y lanza `pide` como 1001: su tabla trae `Home:` en la raíz 2 y ahí crea y borra; lanzar como Ana (no entró)
  da -2; salir da -13 tres veces mientras `pide` vive y 0 en cuanto acaba.
- U6, el registro al disco: `sesiones` tiene su cajón, `Estado/Servicios/sesiones` (usuario 1, 0700; el disco de
  pruebas lo hace con `duenos ... camino=`). Guarda el registro en `registro`: la cuenta (u64) y un anillo de 1.024
  entradas de 40 bytes. Lo nuevo va al disco antes de esperar el siguiente mensaje; al arrancar lee lo que hubiera.
  Si pierde la asa (volver a una instantánea del volumen suelta las de todos, y la prueba de `ficheros` lo hace),
  reabre el fichero sin leerlo y reescribe lo que tiene en memoria. Pruebas: la comprobación del disco en Linux
  (`disco_archivos.py --comprobar`) lee el registro y encuentra los tres intentos del hostil; `selector` (1000, el
  dueño del resto del disco) no abre el registro ni lista el cajón (-12). Sin probar: leerlo al arrancar otra vez
  (cada prueba arranca una vez con un disco nuevo).
- U4, preguntar y guardar: `guardian` se apunta en `sesiones` para atender preguntas (orden 11, PREGUNTAS, con la
  llave por la que se le pregunta). Si un programa pide un servicio sin permiso y `guardian` atiende, `sesiones` le
  pregunta (número, usuario, programa y servicio) y deja la petición en espera sin parar a los demás; con la
  respuesta (orden 12, solo de `guardian`), da la llave o -2. Lo concedido se apunta y se guarda en su cajón
  (`permisos`, una línea "programa servicio"), y se lee al arrancar: no se vuelve a preguntar. `PERMISOS` fijos,
  ahora vacío. Prueba: un `guardian` de prueba (programa nuevo en `virt`) contesta como el usuario: sí a `pide` con
  `eco`, no a lo demás. Se le pregunta una vez por `pide` (las otras dos veces que corre, ya concedido) y una por el
  hostil, que recibe -2. Falta: los permisos del paquete (S8) y la ventana de la pregunta (con el escritorio).
- U4, retirar: orden 13, RETIRAR (solo `guardian`): quita "programa servicio" de lo concedido, reescribe el fichero
  (TAMANO a 0 y lo que queda) y revoca las llaves de ese servicio que tenga ese programa (el permiso se va ya, no al
  pedirlo otra vez). Prueba: `guardian`, al acabar, retira a `pide` el de `eco` (0) y otra vez (-1); el hostil lo
  intenta y recibe -2 (y queda en el registro); en el disco, `permisos` queda vacío. Sin probar: revocar una llave
  viva (en la prueba, `pide` ya la dejó). Para el usuario irá en `Prefs` (U6).
- U5, acciones de administrador: orden 14, ADMINISTRAR, con una acción de la lista cerrada (`ACCIONES`; de momento
  "comprobar", el disco, que hace `archivos`). `sesiones` pregunta a `guardian` (la acción va tras un "*" en la
  pregunta) y espera, sin parar a nadie; `guardian` contesta con usuario y contraseña (orden 15). Vale si el
  usuario es del grupo `admin` (`ADMIN`, provisional: Ana) y la contraseña es buena: la comprueba `sesiones` con
  su volumen, sin abrirlo. Cada acción se confirma (sin plazo de gracia), la hace el servicio que la tiene a su
  cargo y queda en el registro. La contraseña se borra de la memoria al usarla. Prueba: `pide` lanzado como Carla
  la pide y `guardian` da la de Ana: 0, sin problemas en el disco; el hostil la pide dos veces y `guardian` da la de
  Ana mal escrita y la buena de Carla (no es administradora): -2 las dos. Falta: las demás acciones (instalar,
  cuentas, grupos, dueños, volúmenes, hora y red, claves) y la ventana de `guardian`.
- U5: matar un proceso de otro usuario o del sistema es la acción "matar" (su número tras el nombre): con la
  contraseña de un administrador, `sesiones` mata su grupo entero. Prueba: `pide` (como Carla) mata a `eco`, un
  servicio del sistema, con la de Ana: 0, e `inicio` lo relanza (E6); el hostil intenta matar al compositor y
  `guardian` da la de Carla: -2. `MATAR` a uno de otro usuario sigue dando -2 sin preguntar: quien quiera más pide
  la acción.
- U3, el bloqueo con su pantalla: `guardian` tiene el poder de escritorio en el compositor (solo él, por ahora) y, al
  acabar sus preguntas, bloquea la sesión (G5) con una superficie de bloqueo dibujada con el kit: "Sesión
  bloqueada", la frase que eligió la usuaria (S6b: así se sabe que es el sistema), su nombre, un punto por letra y
  el aviso de contraseña mala. Lee el teclado con el mapa del kit y, con Intro, manda la contraseña a `sesiones`
  (VOLVER) sin esperar: mientras llega la respuesta (tras una mala, `sesiones` tarda a propósito) sigue leyendo al
  compositor, que suelta a quien deja de leer (E8). Pinta una vez por tanda de eventos (pintar tras cada tecla
  llenaba la cola y el compositor lo soltaba). Con la buena, quita el bloqueo y espera un sync antes de seguir (si
  no, el compositor podía soltarlo aún bloqueado: negro). `guardian` reintenta apuntarse en `sesiones`, que tarda en
  arrancar. Prueba en virt: el guion de QEMU, al ver la pantalla, deja de teclear lo de siempre y escribe una
  contraseña mala y la buena: desbloquea tras una mala y la captura final ve las ventanas otra vez; tres pasadas
  seguidas bien. Captura en `docs/capturas/u3-bloqueo.png`. Arrancar dos veces con el mismo disco probó también lo
  que faltaba de U6: `sesiones` leyó las 15 entradas del registro de la vez anterior. Falta: la pantalla de entrada,
  bloquear a mano (tecla o tiempo) y la usuaria de verdad de la sesión, no una fija.
- A5, la otra mitad: `read_record_into` lee un registro entero sin comprimir directamente al destino (la ventana del
  cliente), sin el búfer de registro puesto a ceros. Solo si se lee entero desde su principio, no está en la caché
  de escritura, el volumen no está sellado y mide justo lo pedido; si no, el camino de siempre. Descifra el disco
  cifrado y comprueba la suma igual que `read_block` (con la caché de comprobados de F4). Prueba (RedoxFS,
  `tests_a5`): datos al azar (no se comprimen) se leen igual entero y a trozos; con un byte cambiado en el disco, EIO
  por los dos caminos. Medido (7 arranques, contra el mismo banco justo antes): leer 8 MB a trozos de 1 MB, 734 a
  882 MB/s (+20 %); lo demás, dentro del ruido (a trozos de 64 KB no cambia: el trozo es menor que un registro).
- A9, un disco hecho a mala idea no tumba `archivos`: la suma de cada bloque detecta un daño, no una manipulación
  (quien prepara el disco rehace las sumas). Prueba nueva en RedoxFS, `tests_a9`, que hace de atacante: en 2000
  discos cambia campos de nodos (tamaño, nivel de registro, banderas, tipo, bloques, bytes sueltos) con valores
  extremos y los vuelve a sellar, o hace que el contenido al azar de un fichero se lea como un cajón; recorre, busca,
  lee y comprueba todo atrapando pánicos. Encontró 41 pánicos, todos en el mismo sitio: el nivel de registro del
  disco sin comprobar (`BLOCK_SIZE << nivel` daba 0 y una división por cero). Ahora `record_level_checked` da EIO
  por encima de `RECORD_LEVEL` en todos los caminos de `transaction.rs`: 0 pánicos. Ataques dirigidos: un registro
  comprimido rehecho con un largo falso (pánico por índice; ahora EIO) y un cajón dentro de sí mismo (mover un
  cajón no acababa nunca en `check_not_inside`; ahora lleva visitados y un tope). Las dos pruebas fallan sin su
  arreglo (comprobado). Además, una ranura de clave rota ya no hace pánico al abrir un disco cifrado (no abre). Lo
  demás que señalaba la revisión (`BlockAddr::new`, `with_epoch`) solo recibe valores calculados, no del disco, y el
  `count` de los registros de confirmación está acotado (255 bloques). Sin medir: una comparación por lectura.
- A12, `sync_allocator` ya no cambia nada fuera de la transacción antes de confirmar: un bloque de una confirmación
  diferida anterior (en `fs.fresh`) que la transacción suelta se apunta en `forget_on_commit`, y solo al confirmar
  sale de `fresh` y el disco lo olvida. Los nacidos en la propia transacción se olvidan al momento (si falla, nadie
  los usa). El TODO de liberar al fallar no era un fallo: lo reservado vive en la copia del asignador de la
  transacción. Prueba (`tests_deferred`, con un disco que falla al escribir cuando se le pide): una transacción que
  sobrescribe un fichero aún no duradero falla; el fichero se sigue leyendo como antes, y también tras `durable` y
  reabrir, y el disco queda limpio. Sin el arreglo falla ya la primera lectura (el bloque se había olvidado). Sin
  medir: solo cambia cuándo se hace.
- A10, comprobar y reparar al arrancar: antes de atender a nadie, `archivos` comprueba los metadatos (sin leer los
  datos) y, si hay algo mal, repara con `repair.rs` (una transacción: si se va la luz a medias, queda como estaba);
  si algo queda sin arreglar, el disco va de solo lectura. Herramienta nueva, `estropear`, que pone a mano los
  enlaces de un nodo. Prueba en virt: el disco de pruebas sale con los enlaces de `LEEME.txt` a 7, y su montaje
  comprueba que el daño se ve; al arrancar, "1 problemas; reparado, quedan 0" (20 ms en un disco de 7 MB) y la
  comprobación final desde Linux sale limpia. Falta: la reparación como acción de administrador con el servicio en
  marcha (hoy reconstruir el árbol con asas abiertas no es seguro) y medir lo que tarda con un disco grande.
- U4, la llave de plazo: `sesiones` reparte también el permiso de la clase de plazo (S3), como un servicio más (se
  pregunta, se guarda, se deja y se retira), sin entrada de directorio porque no es un canal (`PERMISOS_KERNEL`).
  `inicio` le da ese permiso con el derecho de transferir (a los demás, sus permisos llegan sin él): sus llaves se
  corren un puesto (plazo 1, archivos 2, eco 3, su canal 4, propio 5, archivos sin marca 6). Si mandar una llave
  falla, `sesiones` contesta -1 en vez de nada (el programa esperaba hasta su plazo). Prueba: `pide` la pide (el
  usuario dice que sí), entra con ella en la clase de plazo, la deja y ya no entra; el hostil la pide y recibe -2
  sin llave; `guardian` retira al final los dos permisos de `pide`. El guion de la pantalla de bloqueo teclea la
  segunda contraseña cuando `guardian` ya dijo que la primera era mala, y más despacio (una vez se perdió una
  tecla); `guardian` dice cuántas letras le llegan. Tres pasadas seguidas bien.
- R4, lo que faltaba de S4: el kernel apunta en `placa` las zonas de la ventana de periféricos que usa él (el GIC con
  todas sus partes, el ITS y, en la Pi 3, los controladores de la BCM2836), con los tamaños del árbol, y
  `proyectar_dispositivo` se niega a proyectar a un proceso una página que las toque, aunque una llave de registros
  la cubra. Prueba en el arranque (S4 en `pruebas.rs`): en un espacio de proceso, proyectar el distribuidor del GIC
  y una zona que lo pisa da error; el PL011, que es de un controlador, sí se proyecta.
- K1, lo grande: el planificador guarda en TPIDR_EL1 el hilo que corre en cada núcleo (en cada cambio y al iniciar el
  núcleo; a 0 en el arranque), así que `actual()`, `debe_morir()`, `ttbr0_actual()` y el proceso actual se leen sin
  enmascarar ni tomar el cerrojo del núcleo. `matar` es atómico (se sigue poniendo con el cerrojo de la cola del
  hilo). Medido (7 arranques, contra el mismo banco justo antes): ida y vuelta entre núcleos 62,6 a 55,5 µs (-11 %),
  en el mismo núcleo 76,2 a 66,6 (-13 %), con 4 cargas en el mismo núcleo 109 a 96 (-12 %); varias fuera del ruido.
  Lo que prueba que matar sigue funcionando sin cerrojo: C39 y C43 (matar a uno que gira en otro núcleo).
- La pantalla de bloqueo de `guardian`, más robusta: los glifos se rasterizan antes de bloquear, al teclear solo se
  repinta la caja y el aviso, y tras pintar la primera vez pide un sync y dice "pintada: tiene el teclado"; el guion
  de QEMU empieza a teclear entonces (antes, a veces tecleaba cuando la pantalla aún no tenía el teclado). Queda un
  fallo intermitente: alguna vez (1 de cada 10 pasadas, con K1 y sin él) el compositor suelta a `guardian` por la cola
  llena más de 5 ms (E8) mientras se teclea, y la pantalla queda negra y bloqueada (lo que debe pasar si el
  bloqueador muere).
- La causa: cada evento de entrada es un mensaje aparte (escribir la contraseña son unos 40 en 2 s), la cola de un
  canal admite 16, y en un QEMU cargado basta con que `guardian` se quede sin CPU un tic. E8 está pensado contra
  clientes cualquiera; `guardian` es del sistema, y soltarlo deja la sesión en negro. Ahora el compositor espera hasta
  50 ms a un cliente con el poder de escritorio (`del_sistema`); a los demás, 5 como antes. Ocho pasadas seguidas
  bien. Mejor aún sería juntar los eventos de un cliente en un mensaje (E8, pendiente).
- E7 en `archivos` y `nvme`: mirados y no hace falta cambiarlos. Sus bucles ya cierran las llaves que sobran, duermen
  si no pueden leer y acaban si se cierra el canal; pasarlos al módulo común sería solo cosmético (los dos tienen
  algo propio en el bucle: el guardado con plazo y los anillos).
- U3, bloquear a petición y para quien tiene la sesión: orden 16 de `sesiones`, BLOQUEAR. Solo de un programa cuyo
  usuario tiene la sesión abierta; `sesiones` se lo pasa a `guardian` por su llave de preguntas ("*bloquear" con el
  nombre del usuario). `guardian` bloquea con la frase y el nombre de ese usuario (antes, una usuaria fija), al
  acabar su rato de preguntas (bloquear antes paraba las ventanas de otras pruebas). Prueba: `pide` sin cuenta (dos
  veces) recibe -2; lanzado como Carla con su sesión abierta, 0, y `guardian` bloquea la de Carla y desbloquea con su
  contraseña tras una mala. Además, `guardian` no se apuntaba alguna vez: al arrancar, las altas de `inicio` llenan la
  cola de `sesiones` antes de que lea, y los reintentos se gastaban en el acto; ahora espera medio segundo entre
  ellos, hasta 20 s. Cinco pasadas seguidas bien. Falta la tecla para bloquear (el escritorio) y bloquear por tiempo.
- U3, la pantalla de entrada: la misma de bloqueo en otro modo (`pantalla(None)`): "Entrar en DashOS", primero el
  nombre (se ve) y después la contraseña (puntos); la frase del usuario sale en cuanto se sabe quién es. Con Intro,
  ENTRAR (sin esperar, como VOLVER); si falla, vuelve a pedir el nombre. `guardian` se despide del compositor y quita
  la memoria de la pantalla al acabar cada una (si no, la segunda no tenía dónde ponerse). Al teclear se repinta solo
  la franja que cambia: repintar la pantalla entera por tecla tardaba más de 50 ms en QEMU y el compositor lo
  soltaba. Prueba: tras desbloquear, el guion escribe un usuario que no existe ("Nadie": -2, vuelve a pedir el
  nombre) y después Ana con su contraseña: "entra Ana tras 1 intento malo". El límite de `arranque-virt` pasa de 60
  a 80 s (acaba antes en cuanto salen todas las líneas). Cuatro pasadas seguidas bien. Captura en
  `docs/capturas/u3-entrada.png`. Falta: que sea lo primero al arrancar (hoy, en la prueba, va al final para no parar
  las demás ventanas).
- A8, el asignador sin copia: la transacción ya no copia el asignador del sistema de archivos la primera vez que
  reserva o libera; trabaja sobre él y apunta cada reserva y liberación en `undo`. Al confirmar, se borra el diario;
  si la transacción se suelta sin confirmar (`Drop`), se deshace al revés (lo liberado se vuelve a reservar bloque a
  bloque con `allocate_exact`). La reparación, que rehace el asignador entero, sigue con el suyo propio. Prueba
  (`tests_deferred`): con guardado diferido y sin él, una transacción que crea, escribe y borra y luego falla (por su
  cuenta, o al escribir en el disco) deja el asignador idéntico, lo viejo se lee y lo nuevo se crea después; el disco
  sale limpio. (Al hacerla salió un falso perdido: en modo diferido, comprobar antes de `durable` ve la cuarentena
  como perdida, como dice A11; con o sin el cambio. Se comprueba tras guardar.) Medido en el PC: con 285 trozos
  libres (`perfil`), igual; con el disco fragmentado (`fragmentado`, 4.492 trozos libres), crear y escribir 144 a
  114 µs (-21 %) y borrar 97 a 71 µs (-27 %). Falta de A8: la transacción abierta entre peticiones.
- A8, sin recorridos en el asignador: al liberar, el hermano se calcula (índice más o menos el tamaño del nivel) y se
  mira si está libre, en vez de recorrer el nivel entero; al reservar un bloque concreto, el único candidato es el
  último libre que empieza antes que él (`range`). Y `allocate_exact` con un nivel que no es 0 da `None` en vez de
  `assert` (A9). Prueba (`tests_a8`): 50 secuencias de 3.000 reservas, liberaciones y reservas exactas al azar dan los
  mismos niveles que el algoritmo de antes, operación a operación. Medido en el PC con el disco fragmentado (4.492
  trozos libres): crear y escribir 114 a 53 µs, borrar 71 a 33; con el diario de antes, desde 144 y 97: -63 % y
  -66 %. Con el disco poco troceado (`perfil`), igual.
- A8, rehacer el diario del asignador (cada 1.024 generaciones, `sync_allocator` compara cada trozo con todo el
  diario: cuadrático): medido antes de tocarlo, con 4.492 trozos libres tarda 3,1 ms en el PC. No compensa cambiarlo.
  Se queda la prueba (`tests_a8`): tras rehacerlo con el disco fragmentado y reabrir, el asignador sale igual.
- A13 en `nvme`: antes, quien tuviera su llave veía el disco entero. Ahora cada cliente tiene un tramo de sectores,
  por el nombre que le da `inicio` en el directorio (que `nvme` ya recibía y no usaba): la prueba `disco`, el primer
  MB; `archivos`, desde el MB 1; nadie más. Sin tramo no se abre (-6), y cada lectura o escritura, también por el
  anillo, se mira contra el tramo. Pruebas: `disco` no lee el primer sector de archivos ni uno que empieza dentro y
  acaba fuera (-6); `hostil`, con una llave del canal "disco", no lo abre (-6) ni lee el sector 0. (Las llaves de
  `hostil` se corrieron: siguen el orden en que se nombra cada canal por primera vez, y el disco va antes que archivos:
  ventanas 1, disco 2, archivos 3, sesiones 4, sonido 5, selector 6.)
- Dos fallos intermitentes de las pruebas, arreglados: `pide` pedía un 10 % de la clase de plazo, y si caía en el
  núcleo de los servicios de sonido no cabía (el tope de lo prometido); ahora un 1 %. Y el compositor aún soltaba a
  `guardian` 1 de cada 12 pasadas con 50 ms: ahora le espera hasta 250 (solo a él). Seis pasadas seguidas bien.
- U6, el PID en el kernel: cada proceso que lanza `procesos::lanzar` tiene un número que no se repite (`Control::pid`,
  desde 1). Llamada 37, `proceso_numero` (con la llave del proceso), y 38, `grupo_procesos`: los números de los
  procesos vivos de un grupo, solo con la llave del grupo (así se ven también los hijos sin ficha en `sesiones`, como
  pide S6b). Prueba en el lanzador (C43): cada prueba que lanza tiene número y está en la lista de su grupo, y con una
  llave del tipo que no es (un grupo como proceso, o al revés) da -6: 7 de 7.
- U3, bloquear por tiempo: el compositor da `ext_idle_notifier_v1` (el protocolo de Wayland, MIT, en
  `wayland/protocolos/`) a los clientes de escritorio, de momento solo `guardian`. Cada aviso cuenta desde que se pide;
  una tecla o el ratón lo reinician y, si ya avisó, mandan `resumed`; al vencer, `idled`, una vez. El compositor espera
  sus mensajes como mucho hasta el siguiente vencimiento. Como mucho 16 avisos entre todos: más es un error de
  protocolo. Pruebas: en la escena (`inactividad`: plazos, sin repetir, reinicio por tecla y ratón, destruir uno, de
  más, y nada tras irse el cliente); y en virt, `guardian` pide un aviso de 2 s tras entrar Ana, el guion sigue
  tecleando 3 s y luego para: el aviso llega a los 5,5 s (sin reinicio llegaría a los 2), bloquea la sesión de Ana y se
  desbloquea con una mala y la buena.
- Un fallo intermitente que salió al probarlo (2 de 6 pasadas, con el PC cargado): el compositor soltaba a `guardian`
  en la pantalla de entrada. Era un abrazo mortal: el cliente esperaba sitio en la cola del compositor sin leer la
  suya, y el compositor esperaba a que leyera para seguir. Subir el margen a 1 s no lo arreglaba. Ahora el cliente
  (`ventana_cliente`), mientras espera sitio, recoge lo que le llega: 8 de 8 pasadas bien.
- U3: la pantalla de `guardian` devuelve quién entró o desbloqueó, y el bloqueo por tiempo es para esa usuaria, no
  para una fija. La tecla para bloquear queda para el escritorio (S10b): será él quien la atienda y pida BLOQUEAR.
- A8, la transacción abierta entre peticiones: medida antes de hacerla, no compensa. En el PC (`perfil_agrupado`), 500
  ficheros de 4 KB creados y borrados en una sola transacción frente a una por fichero: crear y escribir 47,9 frente a
  48,2 µs (igual) y borrar 29,7 frente a 25,7 (-13 %). Por partes (`perfil_partes`): buscar un nombre en un cajón de
  500 cuesta 6,8 µs, y buscar y cambiar un nodo 10,2: cerrar la transacción de un cambio pequeño, unos 3,5 µs (copiar
  el camino del árbol, el diario del asignador y la cabecera). Borrar, 30 µs, es casi todo trabajo de la operación
  (quitar la entrada del árbol de nombres, soltar los bloques del fichero), que agrupando no se ahorra. La estimación
  de la revisión (de 1.932 hacia 3.000 o 4.000 borrados/s) era del asignador de antes, que copiaba entero en cada
  transacción. No se hace; quedan las dos pruebas de medida.
- U4, la pregunta al usuario: `guardian` ya no contesta solo. Pone una capa encima de todo (solo él puede: un programa
  no puede pintar una igual por encima), en el centro, con el teclado: "¿Dejas que "pide" use "eco"?", S sí, N no. Lo
  que se pulse en el primer segundo no cuenta (quien estaba escribiendo no contesta sin querer), y sin respuesta en
  30 s es que no. Captura en `docs/capturas/u4-pregunta.png`. Prueba en virt: el guion de QEMU contesta que sí a
  `pide`; al hostil le manda una "s" enseguida, que no cuenta, y luego "n": el hostil no recibe la llave. Al irse una
  capa que tenía el teclado, vuelve a la ventana de arriba (antes se quedaba sin nadie).
- `sesiones`: si ya se le está preguntando al usuario lo mismo (otra copia del programa pidió el mismo servicio), la
  petición nueva espera esa respuesta en vez de preguntar otra vez. Se vio con tres copias de `pide` pidiendo `plazo` a
  la vez: tres preguntas iguales. Y quien pide un servicio espera la respuesta hasta 35 s, no 2 (contesta una persona).
- E8, cola de salida por cliente en el compositor: si la cola de canal de un cliente está llena, lo suyo se guarda
  (`canal::ColaSalida`) y se reintenta en cada vuelta, sin esperar ni parar a los demás. Se le suelta si guarda más de
  64 KB o si no lee nada en 1 s (5 s a `guardian`). Antes se le esperaba 5 ms (250 a `guardian`) con todo parado.
  Prueba en el PC (`cola_de_salida`): orden, uno que lee poco a poco sigue, uno que no lee se suelta a su plazo, y uno
  que recibe mucho, al pasar de 64 KB, con sus llaves fuera.
- `ventana_cliente`: al acabar, cierra su canal y las llaves sin usar, y `guardian` cierra la memoria de cada pantalla y
  de cada pregunta. Antes las dejaba abiertas: una pantalla o una pregunta más, unas llaves menos en su tabla.
- Un fallo del planificador, de antes de esta revisión, que destapó la pregunta: un hilo que se bloquea con un plazo
  que ya ha vencido al planificar (leer con 1 µs, dormir 0) sale de los dormidos como listo, se elige a sí mismo y sigue
  corriendo, pero marcado "listo". Si en ese rato le echa el reloj, el planificador solo vuelve a encolar al que consta
  como corriendo: el hilo no vuelve a ninguna cola y no corre nunca más. Le pasaba a `guardian` 1 de cada 8 arranques
  (leía con 1 µs y luego pintaba, más que un tic); el compositor lo soltaba por no leer y la pantalla de entrada se
  quedaba sin nadie. Se buscó con avisos temporales en el kernel (un hilo vigilado decía "listo en el núcleo 0 hace
  47 s" sin estar en ninguna cola). Arreglo: si sigue el mismo, vuelve a constar como corriendo. Prueba hostil en
  `make prueba` (K11): 4 hilos que esperan 15 veces con el plazo vencido y calculan 12 ms tras cada espera; sin el
  arreglo no acaba ninguno, con él los 4. (No se nota en las medidas: solo pasaba cuando el reloj echaba al hilo justo
  en ese rato, y quien entra en el planificador cada pocos µs retrasa el tic.)
- `make arranque-virt`: límite de 80 a 110 s, por las preguntas (unos 8 s más) y el PC cargado. La pasada buena tarda
  unos 72 s.
- U5, la ventana del selector: quien pide un fichero ya no recibe uno fijo. `selector` lista `C/Privado` (que el que
  pide no puede ver) y enseña una ventana "Abrir" con el kit: la lista, sin nada elegido, y Cancelar. El usuario elige
  con las flechas e Intro, o con el ratón; solo eso se cede, para leer. Un Intro sin nada elegido no abre nada (así un
  Intro de quien estaba escribiendo no abre lo primero), y sin elegir en un minuto, o con Cancelar, no se cede nada.
  Su ventana se lleva el teclado aunque se esté escribiendo: el compositor marca a `guardian` y a `selector` como del
  sistema (por el nombre que da `inicio`; nadie más puede llamarse así), y la regla de S8 no los frena (prueba
  `ventana_del_sistema_toma_el_teclado`). `selector` recibe ahora el canal "ventanas" (llave 1). Prueba en virt: el
  guion pulsa Intro sin elegir (no pasa nada), flecha abajo e Intro; el hostil recibe la asa y lee `nota.txt`, sin
  poder escribir ni ceder. Captura en `docs/capturas/u5-selector.png`. Falta: elegir carpeta (hoy solo `C/Privado`) y
  el doble clic.
- U5: la ventana del selector dice quién pide ("\"hostil\" quiere abrir un fichero"), por el nombre que da `inicio` a
  cada marca del canal "selector" (lo que diga quien pide no cuenta). Una marca sin nombre sale como "un programa".
  Prueba en virt: la línea "ventana de abrir en pantalla, para hostil". Captura nueva.
- C14 (clase de plazo, en `make prueba`) sigue fallando de vez en cuando: 1 de 7 pasadas hoy, con una activación un
  tic tarde (9.985 µs); las otras, de 43 a 726 µs. Ya pasaba antes (1 de 11 en MULTITAREA_LOOP). No viene del arreglo
  del planificador: el hilo de plazo espera su periodo sin plazo de despertar, y el arreglo solo cambia al que sale de
  los dormidos y se elige a sí mismo. Queda por mirar.
- E8, la medida que faltaba: el compositor dice cada 10 s cuántos cuadros ha compuesto, cuánto tiempo y cuántos píxeles
  dañados. En la prueba de virt se compone poco (3 a 7 cuadros por segundo): lo que pesaba no era cuántas veces, sino
  cuánto. Con la pantalla de bloqueo, cada tecla repintaba 4 pantallas enteras: al dar otra vez el mismo búfer, la
  superficie entera tres veces (antes, el sitio de antes y el de ahora, que eran el mismo), y la pantalla de bloqueo
  entera en cada commit. Ahora la superficie entera solo si cambia el búfer o el sitio, la de bloqueo solo la primera
  vez, y al componer, un daño dentro de otro no se compone dos veces. Prueba en el PC (`mismo_bufer_solo_su_dano`):
  el mismo búfer con un píxel dañado compone 1 píxel, y se ve; otro búfer, entero. Medido en virt (3 arranques, por
  cada 10 s): pantallas de bloqueo y entrada, de 5.359 ms componiendo y 81 Mpx a 425 ms y 6,9 Mpx (-92 %); el bloqueo
  por tiempo, de 3.122 ms a 238 (-92 %); al salir las ventanas, de 166 a 135 ms (-19 %). Los cuadros, iguales. Sin
  panel: no hay medida de Linux con la que comparar. El temporizador de cuadro no compensa con este ritmo de cuadros.
- E8, copiar a la pantalla por filas (en vez de una escritura volátil por píxel): medido en virt, igual (dentro del
  ruido): en QEMU pesa componer, no copiar. No se hace.
- Un fallo de 1 en 11 arranques: faltó la línea de `entrada` "lanzar como Carla..., salir después". Sin la salida
  guardada no se sabe la causa; la sospecha es la espera de salir (20 s a que el `pide` lanzado como Carla suelte su
  volumen), que desde U4 puede esperar a que el usuario conteste una pregunta. Ahora espera hasta 40 s. Sin
  comprobar que fuera eso: 10 arranques seguidos bien después (8 antes del cambio y 2 con él).
- C14, mirado un rato más: en 10 pasadas, una con una activación un tic tarde (9.994 µs), y en esa el periodo del hilo
  de plazo no se renovó más tarde que en las buenas. O sea, el reloj despierta a tiempo y el hilo pierde el turno por
  otra cosa (un núcleo ocupado con las interrupciones cerradas, o el reparto). Sin resolver; queda para cuando haya
  medidas en la placa (B5), donde un tic tarde importaría para el sonido.
- A7, la caché de páginas compartida (el usuario eligió la opción "más limpia": rehacer la caché para poder prestarla,
  sin tener los datos dos veces). `archivos` guarda los datos de los ficheros de 512 KB o más por fichero, ya
  descomprimidos y descifrados, en trozos de 512 KB; cada trozo es un objeto de memoria propio (24 MB en total, 48
  trozos, que se crean según hacen falta). Esos datos ya no entran en la caché de bloques (RedoxFS avisa al disco de
  que lee datos de un fichero, `Disk::reading_data`; la caché de bloques baja de 40 a 16 MB y guarda metadatos y
  ficheros pequeños): el servicio no pide más memoria que antes. LEER de un fichero grande sale de ahí. La orden nueva
  PRESTAR (31) da al programa hasta 4 llaves revocables, solo de leer, de los trozos; el programa los proyecta y lee
  sin viajes (`dashos::prestados`). Siguen al día: ESCRIBIR copia también al trozo y TAMANO pone ceros. Dejan de valer
  al cerrar la asa, al pasar el programa de 8 prestados (los suyos más viejos; quien suelta antes de pedir, como
  `prestados`, no lo nota) y al volver a una instantánea, comprobar, o quitar o cerrar un volumen. Al crear un nodo
  se olvidan los trozos de su número (puede ser el de uno borrado). Un trozo prestado no se quita de la caché; si
  todo está prestado, LEER va por RedoxFS como antes.
- Pruebas: en `ficheros` (puede escribir), un fichero de 600 KB se lee entero dos veces con un viaje; lo escrito
  después se ve sin viajar; tras recortar, lo de después son ceros; un fichero nuevo tras borrar el anterior no enseña
  lo viejo; uno pequeño y un cajón no se prestan. Hostil (en `hostil`, con un `C/datos.bin` nuevo de 600 KB): no
  proyecta lo prestado para escribir; lo prestado es el fichero y tras su final hay ceros; pidiendo sin soltar no se
  queda con más de 8 (el primero deja de valer); tras cerrar la asa nada vale; ni cajón, ni fichero pequeño, ni asa
  cerrada.
- Kernel: al quitar una proyección de más de 4 páginas, una sola invalidación de la TLB de todo el proceso (antes, más
  de 32). Con 32 avisos por trozo, quitar los 16 trozos de 8 MB costaba 8,8 ms en QEMU (cada aviso para a todos los
  núcleos); con uno, 1,7 ms. En una placa la diferencia será mucho menor.
- Tamaño del trozo, medido: con 128 KB lo prestado iba a 830-880 MB/s, como LEER; seis llamadas al kernel por trozo
  (prestar, revocar, proyectar, quitar, cerrar...), unos 10 µs cada una en QEMU, pesaban más que copiar. Con 512 KB,
  cuatro veces menos.
- Medido (`make medir-fs`, 5 arranques, medianas, antes → después): leer 8 MB otra vez por LEER en trozos de 64 KB,
  442 → 816 MB/s (+85 %); con lo prestado, copiando a un búfer de 64 KB como `read` en Linux, 1.477 MB/s (Linux,
  1.652; 3,3 veces lo de antes), en 4 viajes; en trozos de 1 MB, 1.734. Peor: leer por LEER en trozos de 1 MB justo
  después de escribir, 886 → 577 MB/s (-35 %): esa primera lectura llena la caché de páginas (una copia más y, la
  primera vez, crear los objetos de memoria). Creo que pesa sobre todo lo segundo, sin comprobar. Lo demás, igual
  (dentro del ruido). Panel al día (paso A7).
- Falta: leer sin viaje entre dos lecturas que no caben en 8 trozos (hoy, un programa que relee 8 MB pide otra vez);
  proyectar ficheros (mmap) sobre esto; y medirlo en la placa.
- A11, el comprobador y la cuarentena: con el guardado diferido, lo soltado espera en cuarentena hasta guardar y
  `check` lo contaba como perdido. Ahora lo cuenta como libre pendiente (la del sistema de archivos y la de la
  transacción). Si la cuarentena miente, lo dice: un bloque en uso sale como "en uso pero libre", uno que el
  asignador ya tiene libre sale con un problema nuevo (`QuarantineFree`, se liberaría dos veces) y uno fuera del
  disco, como fuera. COMPROBAR ya no guarda antes. Y la reparación guarda antes de rehacer el asignador: sin eso, al
  guardar después habría soltado otra vez lo de la cuarentena (en la prueba, 71 bloques libres dos veces).
- Pruebas (`tests_deferred`): comprobar antes de guardar sale limpio y con los mismos usados y libres que después;
  la cuarentena con un bloque en uso, uno ya libre y uno fuera del disco da cada vez su problema; reparar con
  cuarentena deja el disco limpio. Las tres fallan sin el cambio. En `ficheros`, antes de FS8 se escribe y borra un
  fichero de 300 KB y se comprueba en seguida: bien. Que en ese momento la cuarentena tuviera algo es lo esperable,
  pero no está comprobado dentro de QEMU. RedoxFS: 146 pruebas bien.
- A14, las fechas: el kernel sabe la hora del calendario. Al arrancar lee el reloj de la placa si hay uno que sepa
  leer (hoy, el PL031 de QEMU `virt`; la Pi 4 no tiene) y la guarda como base del contador; antes de 2024 no se la
  cree (reloj sin pila). La llamada 39 la da en nanosegundos desde 1970, 0 si no se sabe; con otros valores, error
  (ponerla, cuando venga de la red, pedirá una llave). `archivos` pone esa hora a lo que crea, escribe, recorta, a
  las instantáneas y a los volúmenes; una por petición, y solo en las que cambian algo. Ya no hay `AHORA = 0`.
- Pruebas: en `ficheros`, un fichero nuevo lleva la hora de antes de pedirlo (hasta 2 s después); sin hora, 0. Hostil:
  la llamada con otros valores no pone la hora, y la hora no va hacia atrás. Desde fuera: `ver --fecha` (nuevo) lee
  en Linux la fecha de `Escrito/hola.txt` del disco de la prueba, y tiene que ser de los últimos 10 minutos. Antes
  del cambio, 0.
- Medido (`make medir-fs`, 5 arranques, medianas), con la llamada y sin ella, uno tras otro con el PC igual de
  cargado: todo dentro del ruido, con subidas y bajadas de hasta un 5 %; escribir 8 MB en trozos de 64 KB, 79,1 a
  76,8 MB/s (-3 %), y con guardado, 25,6 a 23,1 (-10 %, con los rangos solapados). Una llamada por escritura de 64 KB
  pesa menos del 1 %. Comparado con las medidas de A7 bajaba hasta un 10 %, también en lecturas que no piden la hora:
  era el PC (dos emuladores de otras sesiones a la vez).
- Falta de A14: ponerla desde la red (con permiso), el controlador de la tarjeta SD, TRIM, la tabla de particiones y
  la lectura anticipada.
- A14, el descarte (TRIM): el disco NVMe tiene la orden 6, descartar, con las mismas comprobaciones que leer y escribir
  (en el anillo también); la hace con Dataset Management si el disco lo admite (ONCS) y, si no, contesta bien sin
  hacer nada. RedoxFS tiene `Disk::discard`: al guardar (`durable`), con la cabecera ya en el disco, avisa de lo que
  sale de la cuarentena, juntando los bloques seguidos. `archivos` lo manda en el envío del guardado siguiente,
  delante de sus escrituras (el servicio los hace por orden: si un bloque soltado se vuelve a escribir antes, se
  descarta y después se escribe). La primera versión los mandaba en el momento, con un viaje más por guardado: guardar
  un fichero de 4 KB costaba un 12 % más y borrar con guardado un 16 %; así, un 5,5 % y un 8 % (`make medir-fs`, 5
  arranques, con y sin, uno tras otro; los rangos no se tocan). Lo que queda es el descarte en sí: en QEMU, cada tramo
  es un `fallocate` en el PC. En la placa habrá que medirlo (algunas tarjetas descartan despacio) y quizá juntar
  más. Linux, por defecto, no descarta al borrar (lo hace `fstrim` de vez en cuando): la comparación con Linux de esas
  dos medidas no es del todo igual.
- No deja ver nada nuevo de los volúmenes cifrados: qué bloques están libres ya se ve en el registro del asignador,
  que va sin cifrar (`AllocList`, `Seal::None`).
- Pruebas: RedoxFS (`discard_after_durable`, con un disco que pone a cero lo descartado al momento): nada se descarta
  antes de guardar, lo descartado se vuelve a usar y, tras un corte, todo está. Descartar antes de tiempo (al entrar
  en cuarentena) rompe esa y otras siete pruebas de cortes. En el disco en bruto (`disco`): 4 KB escritos y
  descartados se leen como ceros; fuera de su tramo, cero sectores, fuera del disco y más de lo que cabe, rechazados.
  En `ficheros`, 256 KB que no se comprimen, borrados y guardados; desde Linux, `disco_archivos.py` comprueba que no
  queda ni un trozo en la imagen. Sin `discard=unmap` en QEMU estaban los 64 trozos de 4 KB; con él, ninguno.
  `make arranque-virt` y `medir-fs` llevan ahora `discard=unmap`. RedoxFS: 147 pruebas bien.
- A14, la lectura anticipada: `archivos` lee del disco por el anillo, y cuando una lectura de datos de un fichero
  empieza donde acabó la anterior y es de un tramo entero (64 KB), deja pedido el tramo siguiente en la otra mitad de
  la ventana del disco (que pasa de 64 a 128 KB): el disco lo lee mientras RedoxFS mira el actual. Lo leído por
  adelantado deja de valer si se escribe algo suyo, y cada guardado lo recoge antes de usar la ventana. Además, al
  leer del disco, un bloque escrito y sin guardar se toma de la caché y no se pisa (antes se podía pisar con lo del
  disco si una lectura lo abarcaba; no se vio que pasara, RedoxFS no lo hace).
- Medido: la prueba de siempre, "leer 8 MB por primera vez", casi no va al disco (el fichero se acaba de escribir y
  sus bloques siguen en la caché de bloques: 24 lecturas de 64 KB de 128). Medida nueva, `leer_8mb_disco`: 8 MB que ya
  estaban en el disco al arrancar (`previo.bin`, lo pone `tools/medir.py`), sin nada en ninguna caché. Con y sin, uno
  tras otro, 5 arranques: 225 a 279 MB/s (+24 %, los rangos no se tocan). Lo demás, dentro del ruido. Linux no tiene
  esta medida en el banco.
- Visto al depurar: al abrir, RedoxFS lee el anillo de cabeceras bloque a bloque (256 lecturas de 4 KB, unos 25 ms en
  QEMU en cada arranque). Siguiente candidato. `tools/medir.py` guarda la salida entera de una vuelta con
  `MEDIR_TEXTO=fichero`.
- Prueba (en `ficheros`): `C/frio.bin` (1 MB de xorshift, puesto en la imagen: frío de verdad) se lee a trozos de 64
  KB escribiendo y guardando otro fichero entre trozo y trozo; sale entero y bien. Con una traza temporal: en esa
  prueba la lectura anticipada se usa 13 veces y una vez se tira por un guardado. `disco` comprueba el nuevo borde de
  la ventana (128 KB).
- Abrir RedoxFS lee el anillo de cabeceras de una vez (1 MB en una lectura, que `archivos` hace en 16 de 64 KB) en vez
  de una lectura por casilla (256 de 4 KB). Abrir, medido en `make arranque-virt` (ahora sale en el mensaje "RedoxFS
  abierto"), 3 arranques: 39,5, 39,0 y 41,6 ms antes; 20,2, 23,7 y 20,9 después (-48 %). Prueba hostil
  (`header_ring_hostile`): basura en casillas vacías y en tres viejas, y una cabecera falsa con una generación enorme y
  la suma mal: abre la buena más nueva, con todo. (Al escribirla salió que romper la cabecera más nueva cuando ya se
  guardó del todo no deja volver a la anterior: sus bloques soltados ya se descartaron. No es un caso de corte de luz:
  un corte durante su escritura sí vuelve a la anterior, y eso lo prueban `single_flush_*`.) RedoxFS: 148 pruebas.
- Abrir RedoxFS, mirado más (traza temporal): de lo que queda, casi todo es la primera lectura del disco; no hay más
  que rascar ahí por ahora.
- A16, NVMe tras un plazo vencido: al esperar una terminada, si llega la de una orden anterior que venció su plazo (su
  identificador no casa), se descarta y se sigue esperando la propia. Antes se tomaba por la de la orden en curso y
  desde ahí todo daba error: un tropiezo del disco era definitivo. Prueba hostil (en `disco`): la orden 7, solo para
  probar, lee como la 1 pero sin esperar la terminada (vence el plazo en el acto, y la terminada llega tarde). Tres
  veces seguidas: las tres vencen, el servicio descarta las tres terminadas tardías (sale en el registro) y después
  leer y escribir siguen dando lo que toca; fuera de su tramo, también rechazada. No se probó con el código de antes
  (que con una terminada de otra orden devolvía error).

### 2026-11-11 (tras reiniciar shiva)

- A15, los huecos del disco: mirado. Desde A13 solo abren el disco los programas con tramo (`disco` y `archivos`), y
  `inicio` da a cada programa la misma marca al relanzarlo (su número más 1): vuelve a su hueco y no se pierden. Lo
  que sí fallaba: el relanzado pedía su anillo y el servicio se lo negaba ("ya tiene uno"), así que iba sin anillo,
  más lento. Ahora un anillo nuevo sustituye al viejo: se quita, con lo que tuviera a medias, y deja de atenderse.
  Prueba hostil (en `disco`): pedir el anillo otra vez da uno nuevo con el que 16 lecturas van bien, y una lectura
  puesta en el viejo, con aviso, no se atiende en 50 ms. Lo demás de A15 (varias órdenes NVMe a la vez, COMPROBAR
  por trozos) sigue pendiente.
- A7, la causa de la bajada al leer en trozos de 1 MB justo después de escribir (886 a 577 MB/s), comprobada: es
  crear la memoria de los trozos de la caché de páginas la primera vez. Con los 48 trozos creados al arrancar (solo de
  prueba), 3 arranques con y sin: leer en trozos de 1 MB tras escribir, 515 a 791 MB/s (+53 %); leer del disco en
  frío, 214 a 317 (+48 %); lo demás, igual. Con trazas: crear un trozo cuesta unos 215 µs (crear la memoria 150-250,
  proyectarla unos 22, la copia de solo leer unos 11) y llenarlo unos 510; en `medir-fs` los 48 se crean durante la
  medida, así que cada lectura de 8 MB paga 16 creaciones. Cuando ya existen se reutilizan sin ese coste.
- Poner las páginas a cero con `dc zva` (de línea en línea) en vez de byte a byte: probado, no cambia nada (leer en
  trozos de 1 MB, 564 sin y 554 con, en el ruido); lo caro de crear no es poner a cero. Descartado. (Con él, una de
  cuatro pasadas de `make prueba` falló en el paso 13 de `grupos`, el cupo del grupo no volvía a 0 en el acto tras
  matarlo; no se vio sin él en una pasada; sin investigar, queda apuntado.)
- No se cambia: crear los trozos al arrancar quitaría ese coste a costa de 24 MB desde el arranque (hoy se crean
  según hacen falta) y unos 10 ms más de arranque en QEMU. Para decidir con el usuario. Otra vía: abaratar
  `memoria_crear` en el kernel (hoy pide las páginas de una en una, con el cerrojo del repartidor cada vez).
- El fallo del paso 13 de `grupos`, mirado: era una carrera que ya estaba. Lo de un proceso que acaba se devuelve
  cuando su núcleo recoge sus restos (`planificador::recoger`, en la siguiente llamada al kernel de ese núcleo o al
  quedar ocioso), no al acabar; la prueba miraba el uso del grupo en el acto tras matarlo y, rara vez, el núcleo del
  hijo aún no lo había recogido (con `dc zva` cambió el reparto de tiempos y salió). No es una fuga: la memoria sigue
  ocupada de verdad hasta entonces. La prueba espera ahora hasta 200 ms a que vuelva a 0 (y falla si no). Sin cambios
  en el kernel, 3 pasadas bien; con la prueba nueva, 2 más.
- La prueba de A16 también fallaba a veces: QEMU a veces acaba la lectura en el acto y el plazo no vence (una vez
  venció 1 de 3). La línea esperada pedía "3 de 3"; ahora basta con que venza una vez, y la cuenta va en otra línea.
- Por qué cuesta crear un trozo de 512 KB, medido dentro del kernel (traza temporal): 122 µs de mediana, de ellos 115
  de poner las 128 páginas a cero. En QEMU cada escritura se emula, y `dc zva` no lo abarata (visto antes). Sacar las
  páginas en tandas de 64 con una sola toma del cerrojo del repartidor: 164 µs frente a 161, nada; deshecho. (Probado
  también sin poner a cero, solo para medir: `archivos` no abre RedoxFS, cuenta con memoria nueva a cero.) Lo que
  quedaría es tener páginas ya a cero, preparadas por los núcleos ociosos: en esta medida ganaría como mucho un 13 % en
  la primera lectura, a cambio de tocar el repartidor del kernel. No se hace; en la placa, poner a cero 512 KB cuesta
  mucho menos que en QEMU, y habrá que medirlo allí.
- A14, la tabla de particiones. Antes el sistema de archivos empezaba siempre en el MB 1. Ahora el servicio de disco
  lee la tabla al arrancar (`usuario/src/particiones.rs`, nuevo): MBR (la partición de DashOS, tipo 0x7F) o GPT (tipo
  d0a18dbf-bbf4-4b0e-8627-ce5006b50017, con los CRC de la cabecera y de la lista; la copia del final no se mira). De
  ahí salen los tramos: `archivos`, la partición de DashOS; `disco`, el primer MB sin llegar a la primera partición
  (con GPT, nada). Y cada cliente ve su tramo como su disco entero: el servicio suma el principio del tramo, la
  respuesta da los sectores del tramo y no se puede ni nombrar lo de fuera (antes, -6 fuera del tramo; ahora, -2
  fuera del disco). `archivos` ya no lleva el MB fijo (`INICIO` fuera). Los discos de prueba y de `medir-fs` llevan un
  MBR con la partición desde el MB 1; `ver` acepta `tabla` en vez del desplazamiento y `--comprobar` lo usa.
- Ante una tabla que no cuadra no se adivina: con entradas fuera del disco, de longitud cero, que tapan el sector 0,
  que se pisan, dos de DashOS, más de 128 en uso, cabecera rara o CRC que no casa, nadie recibe tramo, ni la prueba en
  bruto (que escribe). Con la lista estropeada, el fallo es el CRC aunque una entrada tampoco valga.
- Pruebas hostiles: 8 en `archivos/herramientas` (`cargo test`, ahora en `make prueba`): MBR y GPT buenas (sectores de
  512 y 4096, arranque FAT delante como en la Pi), 8 MBR rotos, 14 cabeceras GPT rotas con su CRC rehecho, entradas
  fuera, pisadas y repetidas, un disco que no se deja leer y 20 000 tablas estropeadas al azar (nunca un pánico; lo que
  se acepta cabe en el disco). Quitando una a una tres comprobaciones (CRC de la lista, pisadas, sector 0), cada vez
  falla una prueba. En `disco`: ve 2048 sectores, el primero de `archivos` y uno que cruza dan -2, y también uno tan
  grande que al sumarle el principio del tramo daría la vuelta. En QEMU, a mano, un disco con dos entradas pisadas: el
  servicio dice "la tabla de particiones no vale (Pisadas): no se abre a nadie", `disco` y `archivos` no abren, el
  sistema sigue (el compositor compone) y el disco sale igual que entró, byte a byte.
- Rendimiento: no cambia el camino (una suma por orden); `make medir-fs VUELTAS=1` funciona con el disco nuevo y da lo
  de siempre (leer del disco en frío 265 MB/s, en el ruido de 279).
- A7, la caché de páginas se crea al arrancar (lo decidió el usuario, 2026-11-11): `archivos` crea los 48 trozos de
  512 KB (24 MB) antes de atender a nadie (`Paginas::crear_todos`); si no hay memoria para todos, los que falten se
  crean según hagan falta, como antes. Cuesta 11 ms de arranque en QEMU (línea nueva en `arranque_virt.txt`). Medido
  con `make medir-fs VUELTAS=3`, antes y después: leer en trozos de 1 MB, 541 a 749 MB/s (+38 %); leer 8 MB por primera
  vez, 343 a 410 (+20 %, por encima de Linux, 401); del disco en frío, 277 a 300 (+8,5 %); lo demás, dentro de ±4 %,
  el ruido. Panel de medidas al día (fila A7b y el grupo Ficheros con esta medida).
- A15, varias órdenes NVMe a la vez. Antes el servicio de disco atendía el anillo de una en una, esperando cada
  terminada. Ahora junta hasta 8 envíos seguidos (lecturas, escrituras y descartes) y los manda al disco a la vez,
  cada uno con su lista de páginas (`M_LISTA`, 512 bytes por orden) y su tramo de descarte; las terminadas van al
  anillo en su orden cuando acaba el lote. Un lote se corta en lo que tiene que ir detrás (`choca`: el mismo sector y
  uno escribe o descarta; el mismo sitio de la ventana y uno lee) y en vaciar, que va solo y con nada en vuelo. Si una
  terminada no llega a tiempo, las que faltan dan error y su terminada tardía se descarta (A16). La cola del NVMe ya
  no se puede desbordar: se lleva por dónde va el controlador (la cabeza que dice cada terminada) y, llena, se
  contesta "sin sitio". `Nvme::orden` queda como `enviar` más `terminada`.
- Prueba hostil (en `disco`, nueva línea): 21 envíos de una vez que se pisan a propósito (escribir y leer el mismo
  sector, dos escrituras seguidas, descartar y escribir, dos lecturas al mismo sitio de la ventana, escribir desde un
  sitio y leer encima, y 8 lecturas tras una escritura de 8 sectores): todo bien y en orden. Quitando la regla de
  orden, la prueba da MAL; con una traza, los lotes salen de 2 a 8 (42 de 8 en un arranque).
- Medido, 5 vueltas con y 5 sin, seguidas (el PC con carga 4 de otras sesiones; con 3 vueltas, la lectura prestada
  pareció bajar un 15 %, y con 5 sale igual, 1492 las dos: era ruido): leer del disco en frío, 257 a 299 MB/s
  (+16 %); crear 4 KB guardado, 320 a 336 (+5 %); escribir guardando cada trozo, 20,8 a 25,3 (las vueltas de antes,
  muy dispersas); lo demás, igual. Poco, porque `archivos` casi nunca tiene más de dos cosas en el anillo; servirá
  cuando haya varios clientes y con discos de verdad, que sí trabajan en paralelo.
- De A15 queda COMPROBAR por trozos (un comprobador que se pueda parar entre vueltas del bucle sin perder la foto del
  disco; hoy son 19 ms al arrancar con 7 MB, y crecerá con el disco) y, de fondo, hilos en el servicio (fase 8). Los
  topes: `nvme` admite 4 clientes y solo hay dos con tramo; `archivos`, 64.
- U5, lo que faltaba del selector: cajones y doble clic. La ventana empieza en `C` (lo del usuario) y enseña el camino
  ("C/Privado"); elegir un cajón entra en él, "../" vuelve, y por encima de `C` no se sube (en `C` no hay esa fila).
  Va por asas: cada cajón abierto se guarda, y lo elegido se busca por la asa de su cajón y su nombre, en un solo
  paso, y se mira su tipo en ese momento (pudo cambiar desde que se listó): solo un fichero se cede. No se enseñan
  los nombres ".", "..", vacíos o con '/', ':' o '|': '|' separa las filas de la lista del kit, y uno con ese
  carácter correría las de después, con lo que se cedería otro fichero del que se ve (en el disco de prueba,
  `C/raro|nombre.txt`; la ventana dice "1 nombres raros sin enseñar"). Hasta 32 cajones de hondo.
- El doble clic, en el kit, como en MUI (`atr::DOBLE`, `MUIA_Listview_DoubleClick`): dos clics seguidos sobre el
  mismo objeto y con el mismo valor (en una lista, la misma fila) en 500 ms, con la hora de los eventos de Wayland
  (`Interfaz::raton_hora`); Intro en una lista también lo da. El selector abre con `DOBLE`: un clic solo marca la
  fila. Prueba en el kit (`doble_clic_en_una_lista`, y las del kit van ahora en `make prueba`): sin hora no hay doble,
  ni en filas distintas, ni con 600 ms; el tercer clic no hace otro con el segundo; vale con la hora dando la vuelta;
  e Intro. En virt, el guion: Intro sin nada, flecha abajo e Intro (entra en `C/Privado`), Fin e Intro (cede
  `nota.txt`); el hostil la lee y nada más. El doble clic con el ratón de verdad no se prueba en QEMU (el guion no
  sabe dónde queda la ventana). Captura nueva en `docs/capturas/u5-selector.png`.

### 2026-11-12

- R11 (S2), lo que quedaba: `its.rs`. Cada orden al ITS se esperaba con hasta un millón de lecturas, con su cerrojo y
  las interrupciones cerradas, y se llega desde un programa (asociar o soltar una interrupción, `its::encender`).
  Medido en QEMU con GIC v3 y una orden que el ITS no lee: 195 ms así. Ahora la espera tiene tope de 1 ms (`PLAZO_US`;
  un ITS lee sus órdenes en microsegundos) y, si vence o la cola se para, el ITS queda por roto: no se le manda nada
  más y sus MSI se niegan (`msi` da nada), en lugar de seguir con una cola de la que no se sabe por dónde va. La espera
  de "quieto" al arrancar, con tope de 10 ms. Prueba hostil (`prueba_its`, solo con GIC v3): arranca el ITS, enciende
  una LPI (unos 82 µs en QEMU), atasca la siguiente orden (no mueve CWRITER: `its::ATASCAR`, solo en la imagen de
  pruebas) y mide: vence en 1.048 µs (antes, 195.127, y después seguía dando MSI con la cola descolocada); la
  siguiente no espera (8 µs) y no hay MSI. `make prueba-virt` va ahora con GIC=3 (el v2 lo prueban `make prueba` y
  `make arranque-virt`); 3 pasadas, todas con la línea bien. `make arranque-virt` también arranca con GIC=3 (probado a
  mano antes del cambio: ITS, MSI-X de xHCI y NVMe y todo lo demás, salvo el sonido, que no se puso).
- En esas pasadas salieron dos fallos sueltos que no tocan el ITS: C14 (el de siempre, 1 de 4) y el paso 5 de
  `grupos`, la misma carrera que el 13 (lo de un proceso que acaba se devuelve al grupo cuando su núcleo lo recoge).
  El 5 espera ahora como el 13, hasta 200 ms (`vuelve_a_cero`).
- C14, el fallo de vez en cuando, con causa: `crear_plazo` ponía el hilo de plazo en la cola de su núcleo (el 1, con
  tres hilos justos ocupados) sin avisarle, y el núcleo no lo veía hasta su siguiente tic, hasta 10 ms después, casi
  al final del primer periodo. Al acabar ese primer turno el periodo ya se había renovado, así que le tocaba otro en
  el acto (primer hueco, 1 ms en lugar de 10) y el tercero llegaba antes de tiempo. La prueba se saltaba el primer
  hueco, pero el segundo salía con 455 a 948 µs de desviación, al borde del tope de 1.000. Medido con una traza
  temporal (C14 repetida 30 veces en un arranque): antes, el primer hueco de 1 ms en las 29 rondas limpias y la peor
  desviación de 455 a 948 µs; con el aviso (`gic::avisar` con `SGI_PLANIFICAR` al crear), primer hueco de 10 ms y peor
  desviación de 11 a 507 µs (casi todas por debajo de 50; las de 300 a 500 son retrasos sueltos de QEMU que el
  siguiente compensa). La prueba mide ahora desde el primer hueco (con el fallo de antes daría unos 9.000 µs). Si el
  "un tic tarde" (9.985 µs) visto antes venía de aquí no se ha podido ver: no se repitió. `make prueba` 5 veces bien
  (C14: 108 y 96 µs de desviación en las dos últimas), `make arranque-virt` bien.
- El usuario pidió (2026-11-12) un centro de control tan potente como el de DMS (Dux Modular Shell), y capturas de
  todo lo de la interfaz. Diseño en `docs/diseño/S10c-centro-de-control.md`. La pieza de R19 que quedaba, Prefs de
  permisos, se hace como su primera fase (C1): la ventana de Ajustes con la categoría Permisos. Va la siguiente.
- S10c, fase C1: el centro de control, `centro` (`usuario/src/bin/centro.rs`). Una ventana con el kit: a la izquierda
  las categorías, a la derecha la elegida. De momento, **Permisos**: lo concedido a cada programa ("pide · eco") y
  quitarlo, con el botón Quitar o con Supr (se revoca en el acto); Esc o Cerrar la cierran. Por su canal "centro" solo
  se le pide que abra una categoría ("abrir permisos", el enlace directo de S10c); si el pedido trae llave, contesta al
  cerrarse. `sesiones` gana la orden 17, CONCEDIDOS (por páginas de 7), y RETIRAR se deja también al centro: las dos,
  solo para quien `inicio` llama "centro" o "guardian" (y "entrada", que hace de guardian en las pruebas). `centro` va
  en la tabla de `inicio` antes que `guardian`: el primero que nombra un canal es su servidor.
- Prueba en virt: `guardian` retira él el permiso de `plazo` y, 3 s después de avisarlo, pide al centro abrir Permisos
  y espera a que se cierre (lo siguiente es bloquear la sesión). El guion deja de teclear al ver el aviso, así la
  ventana se lleva el teclado (S8); dentro, flecha abajo y Supr, y Esc. El centro enseña 1 concedido, lo quita (0;
  quedan 0) y se cierra; el disco acaba con el fichero de permisos vacío, como antes. Hostil: el cliente hostil pide
  CONCEDIDOS y recibe -2 y 16 bytes sin nada. Captura en `docs/capturas/s10c-centro-permisos.png`. El ratón (clic en
  Quitar) no se prueba en QEMU: el guion no sabe dónde queda la ventana.
- S10c, C1, segunda categoría: **Seguridad**, el registro de `sesiones` (negativas y fallos, lo último primero, con
  "hecho", "negado" o "error N"), solo de lectura (Quitar, apagado). El registro lo leía solo `guardian`; ahora también
  el centro (`REGISTRO`, con la lista `VEN_PERMISOS`). Se cambia de categoría en la barra lateral (Tab para llegar a
  ella con el teclado). Prueba en virt: tras quitar el permiso, Tab y flecha abajo; el centro dice "Seguridad, con
  negativas del hostil: sí; 18 entradas". El hostil sigue sin poder leer el registro (su prueba de antes). Pendiente:
  con varias sesiones, que cada usuario vea solo lo suyo (hoy hay una sesión cada vez). Captura en
  `docs/capturas/s10c-centro-seguridad.png`.
- S10c, C1, tercera categoría: **Acerca de** (como la de DMS): la versión, cuánto lleva encendido, la fecha y hora
  (UTC, del reloj de la placa de A14, pasada a fecha con el algoritmo de días civiles de Howard Hinnant, de dominio
  público) y cuántos procesos hay (la cabecera de `PROCESOS`). Solo se lee. Prueba en virt: una flecha abajo más tras
  Seguridad; el centro dice "Acerca de: DashOS, versión 0.0.1; Encendido hace 0 min 54 s; Fecha y hora: 2026-11-11
  23:50 (UTC); Procesos en marcha: 15". Captura en `docs/capturas/s10c-centro-acerca.png`.
- S10c, C1, cuarta categoría: **Discos**, de solo lectura: cuánto mide el disco del sistema, cuánto está ocupado y
  cuánto libre (INFO de `archivos`). El centro recibe el canal "archivos" con una ficha mínima (usuario 1000, sin
  derechos, encerrado en `C`); comprobar el disco y las instantáneas, que piden administrador, quedan para cuando la
  prueba no tenga a `guardian` esperando al centro (las dos cosas se esperarían). Ojo, lo que costó un arranque: las
  llaves de un programa van en el orden en que se nombró cada canal en todo el sistema, no en el de su lista; con
  "archivos", las del centro pasan a ser ventanas 1, archivos 2, sesiones 3 y centro 4. Prueba en virt: "Discos:
  Disco del sistema: 7,0 MB; Ocupado: 2,5 MB (36 %); Libre: 4,4 MB". Captura en `docs/capturas/s10c-centro-discos.png`.
- S10c, la prueba hostil del propio centro: el cliente hostil recibe el canal "centro" (su llave 5; las de sonido y
  selector pasan a 6 y 7, por el orden en que se nombran los canales) y le manda seis pedidos que no valen (una
  categoría con camino, "abrir" solo, vacío, 200 bytes de basura, la buena con algo detrás, la buena con mayúscula),
  cada uno con su llave de vuelta, una inundación de 160 mensajes con 8 llaves cada uno y otro pedido malo después.
  El centro contesta -1 a todos sin abrir ventana (antes contestaba 0 a cualquiera con llave) y sigue contestando
  tras la inundación; de los pedidos malos solo dice los tres primeros. Después, `guardian` lo abre bien, como antes.
- S10c, el principio de C3: **Apariencia** en el centro. Modo claro u oscuro y color de acento (violeta, azul, verde,
  naranja); el esquema sale del color con `Tema::desde_color` del kit (Material 3) y se elige con Intro o doble clic
  (`atr::DOBLE` en la lista de la derecha; en Permisos, Intro ya no quita nada: eso es Supr). Se ve en el acto, de
  momento solo en la ventana del centro: llevarlo a todos los programas pide un servicio de tema, y guardarlo, un sitio
  en Prefs. Prueba en el kit (`contraste_con_cualquier_color`, en `make prueba`): con 8 colores escogidos (negro,
  blanco y amarillo puro incluidos) y 300 al azar, en claro y en oscuro, cada texto sobre su fondo da 4,5:1 o más
  (WCAG AA). En virt: Tab a la lista, dos abajo e Intro; "Apariencia: oscuro, color Violeta" y la ventana se repinta
  oscura. Captura en `docs/capturas/s10c-centro-apariencia.png`.
- S10c, la apariencia se guarda: en `Prefs/apariencia` desde la raíz del centro (`C/Prefs/apariencia` en el disco), una
  línea como "oscuro violeta". Si no están, el centro crea el cajón (0700) y el fichero (0600). Se lee al abrir la
  ventana; si no está o no se entiende (no es UTF-8, otro modo, un color que no existe, algo de más, más de 64 bytes),
  se queda la de serie. La sesión con `archivos` va en `Sesion`, con su ventana proyectada mientras dura. Pruebas: el
  disco de prueba trae la preferencia en basura (bytes que no son UTF-8 y "magenta"), y el centro dice "no se
  entiende; se queda la de serie"; al elegir el modo oscuro la guarda (15 bytes), la relee bien, y el disco leído en
  Linux tiene "oscuro violeta". El hostil (otro usuario, encerrado en `C`) no la abre (-12). El guion del selector
  baja dos veces para llegar a `Privado`: `Prefs` va antes.
