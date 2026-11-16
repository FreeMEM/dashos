# Referencia del kernel de DashOS

Lo que un programa ve del kernel tal como está hoy en el código. El diseño, el porqué, está en `docs/diseño/` (S1 a
S3). Este documento dice qué hay y cómo se usa. Si no coincide con el código, manda el código y hay que corregir esto.

Estado: 2026-11-03, tras C30. Todo es **provisional**. En S11 se decidió que los programas no llamarán al kernel a
mano, sino a través de una biblioteca. Los números y el orden de los argumentos pueden cambiar.

Fuentes: `kernel/src/llamadas.rs`, `objetos.rs`, `elf.rs`, `planificador.rs`, `interrupciones.rs`, `espacio.rs`, y
la biblioteca de usuario `usuario/src/lib.rs`.

## Procesos

Un proceso tiene su espacio de direcciones, su tabla de llaves y su orden de bytes. Hoy cada proceso tiene un solo
hilo. Lo arranca el kernel desde un ELF.

Cada espacio tiene un ASID de 16 bits propio mientras vive; dos procesos vivos nunca lo comparten. Cuando sale el
último hilo, el proceso se libera en ese momento: su memoria, sus tablas y sus llaves.

### Formato del ejecutable

ELF64 para AArch64 (`e_machine` 183), en cualquiera de los dos órdenes de bytes:

- **Little-endian** (`EI_DATA` = 1): lo nativo de DashOS.
- **Big-endian** (`EI_DATA` = 2): la caja Amiga y sus bibliotecas.

El proceso corre en el orden de bytes de su ELF. El kernel pone `SCTLR_EL1.E0E` en cada cambio de hilo, así que
procesos de los dos órdenes conviven en el mismo núcleo. Las instrucciones son siempre little-endian, como manda
AArch64.

Hay dos tipos de ejecutable:

| Tipo | Dónde va | Pila |
|---|---|---|
| `ET_EXEC` | En sus direcciones. Todo debe acabar antes de 2 GB − 8 MB | Termina en `0x8000_0000` |
| `ET_DYN` (PIE estático) | Base al azar entre 128 y 256 GB, alineada a 64 KB | Tope al azar entre 256 y 384 GB |

Reglas comunes:

- Se cargan los `PT_LOAD`. Si un segmento tiene `X`, es código (lectura y ejecución). Si tiene `W`, son datos
  (lectura y escritura, sin ejecución). Si no, es de solo lectura.
- Se rechaza el ELF si:
  - un segmento tiene `W` y `X` a la vez;
  - dos segmentos comparten página;
  - algo cae en los primeros 64 KB;
  - la entrada no está en un segmento con `X`;
  - `e_phentsize` no es 56 o hay más de 64 cabeceras de programa.
- Si hay `PT_INTERP`, el ELF se rechaza. No hay enlazador dinámico.
- **El kernel no aplica reubicaciones.** Un PIE aplica sus `R_AARCH64_RELATIVE` al arrancar, y tienen que caer en un
  segmento escribible. Su base la saca de su propio contador de programa (`adr`/`adrp`).
- La pila mide el `p_memsz` del `PT_GNU_STACK`, redondeado a páginas, entre 4 KB y 8 MB. Sin `PT_GNU_STACK`, mide
  64 KB.
- Al arrancar, `SP` apunta al tope de la pila y `PC` a la entrada. Los demás registros valen cero. No hay
  argumentos, entorno ni vector auxiliar.

### Mapa de direcciones de un proceso

El espacio de usuario va de 64 KB a 512 GB (`1 << 39`). Los primeros 64 KB no se proyectan nunca, con una excepción:
un proceso con el permiso de página 0 (la caja Amiga, H4) puede proyectar con la llamada 49 una página de solo lectura
en la dirección 0 (ExecBase en la dirección 4); el resto de los 64 KB sigue cerrado también para él, y el kernel no
copia nunca desde la página 0 ni hacia ella. Sin PAN (la A72 no lo tiene), un puntero nulo del kernel mientras corre la
caja leería esa página en vez de fallar; escribir en ella, no. Hoy está así:

| Tramo | Uso |
|---|---|
| 0 – 2 GB | Ejecutables `ET_EXEC` (desde `0x40_0000`) y su pila (hasta `0x8000_0000`) |
| 0 – 4 GB | En la caja Amiga, la memoria del Amiga tal cual (S3). Ningún código nativo de la caja va aquí: es un PIE |
| desde 4 GB | Libre. El runtime de Dash pone ahí su montón, con `memoria_crear` y `memoria_proyectar` |
| 128 – 256 GB | Imagen de los PIE |
| 256 – 384 GB | Pila de los PIE |
| 384 – 512 GB | Libre |

El programa decide dónde proyecta su memoria. El kernel no le busca sitio.

### Fin de un proceso

- `salir(código)` termina el hilo que llama. El proceso acaba con su último hilo, con el código de ese.
- `proceso_salir(código)` (44) termina el proceso entero, con todos sus hilos.
- Hilos de usuario: `docs/diseño/hilos-de-usuario.md`. Un hilo creado con `hilo_crear` cuesta 20 KB al grupo hasta
  que alguien se une a él (o acaba el proceso). En x86-64, 24 KB: su estado de coma flotante (XSAVE) ocupa 4 KB.
- Una excepción del programa, en cualquiera de sus hilos, termina el proceso entero, no el sistema. Por ejemplo, un fallo de página, una instrucción
  ilegal o una alineación incorrecta. El kernel escribe en la consola la clase de la excepción, la dirección de la
  instrucción y la dirección del fallo.
- Al morir el último hilo se liberan la tabla de llaves, el espacio de direcciones y las páginas que solo usaba ese
  proceso (C18).

### FPU y SIMD

Cada hilo tiene sus propios registros de coma flotante y SIMD: `q0`–`q31`, `FPCR` y `FPSR`. La primera vez valen cero
y `FPCR` = 0, es decir, redondeo al más cercano y sin excepciones de coma flotante. El kernel los guarda solo cuando
otro hilo del mismo núcleo usa la FPU (C28). No hay SVE.

### Cachés

Desde H4, EL0 puede leer `CTR_EL0` y limpiar o invalidar las cachés por dirección virtual (`dc cvau`, `dc cvac`,
`dc civac`, `ic ivau`; `SCTLR_EL1.UCT` y `UCI`), como en Linux. La MMU lo comprueba con los permisos del proceso.
Lo usa un JIT (ver la llamada 8). `dc ivac` y las operaciones por conjunto y vía siguen prohibidas: el proceso muere.

### Planificación

Los procesos de usuario van en la clase justa: reparto por tiempo virtual, con un tic de 10 ms. Un hilo que despierta
empieza como mucho medio tic por debajo del mínimo de su núcleo y, si va por delante del que corre, se adelanta al
momento (M1 y M3, `docs/MULTITAREA_LOOP.md`). La clase de plazos de S2 la usan el kernel y el sonido (llamada 28, con
el permiso de plazo que reparte `inicio`, S3). Lo prometido a la clase en un núcleo, contando los hilos que duermen o
esperan, no pasa del 80 % (K8). Un
proceso corre en el núcleo donde se creó y no cambia de núcleo.

### El programa de inicio

En la imagen normal, el kernel lanza un solo proceso, `inicio` (C47), en el grupo raíz. Le da las llaves de arranque,
las únicas que da el kernel:

- La 1: el paquete de arranque, un objeto de memoria de solo lectura. Dice qué es cada una de las demás llaves y trae
  los ELF de los servicios. Su formato está en `kernel/src/inicio.rs`.
- Después, los permisos DMA (uno por aparato con DMA), el registro del sistema, el permiso de plazo, el de máquinas
  virtuales (H9), el de página 0 (H4), el de apagar (llamada 51), el del aviso de memoria (llamada 53), la memoria de
  la VideoCore y, para
  cada aparato del árbol que conoce el kernel (serie, SD, USB, buzón de la VideoCore, y la V3D si contesta), sus
  registros (en páginas enteras) y su interrupción.

`inicio` lanza cada servicio en su grupo, con copias de las llaves de sus aparatos y los canales que comparte con
otros servicios, y se queda esperando. Hoy son la consola (C48), el buzón, el USB, el compositor y la GPU (C49, C50); lo que
ve un programa de ellos está en `servicios.md`. Ningún otro proceso recibe nada del kernel: todo lo
que tenga le llega desde `inicio`.

## Llamadas al sistema

Se entra con `svc #0`. El número va en `x8` y los argumentos en `x0`–`x4`. El resultado vuelve en `x0`, y en
algunas llamadas también en `x1`. Un resultado negativo es un error (ver la tabla de errores).

Las direcciones que se pasan son del propio proceso. Los tamaños van en bytes.

El kernel copia lo que lee o escribe en la memoria del proceso con accesos sin privilegio (`LDTRB`/`STTRB`, C31). La
MMU los comprueba con los permisos de EL0. Así, el kernel no puede leer por el proceso lo que el proceso no puede leer,
ni escribir donde él no puede escribir, como su código. Si una dirección no está proyectada o no tiene permiso, la
llamada devuelve `E_ARGUMENTO` y el kernel sigue.

| n | Llamada | Argumentos | Devuelve |
|---|---|---|---|
| 1 | `escribir` | `x0` texto, `x1` longitud | Bytes tomados: como mucho 256; hay que repetir con el resto |
| 2 | `salir` | `x0` código | No vuelve |
| 3 | `canal_crear` | — | `x0` y `x1`: las llaves de los dos extremos |
| 4 | `canal_escribir` | `x0` llave, `x1` datos, `x2` longitud, `x3` tabla de llaves, `x4` cuántas | 0 |
| 5 | `canal_leer` | `x0` llave, `x1` búfer, `x2` capacidad, `x3` tabla de llaves, `x4` capacidad de la tabla | `x0` bytes, `x1` llaves recibidas |
| 6 | `cerrar` | `x0` llave | 0 |
| 7 | `memoria_crear` | `x0` tamaño (hasta 16 MB) | Llave de un objeto de memoria |
| 8 | `memoria_proyectar` | `x0` llave, `x1` dirección (alineada a 4 KB), `x2` permisos: 1 leer, 2 escribir, 4 ejecutar | 0 |
| 9 | `registros_proyectar` | `x0` llave de registros, `x1` dirección (alineada a 4 KB) | 0 |
| 10 | `interrupcion_asociar` | `x0` llave de interrupción, `x1` llave de un extremo de canal | 0 |
| 11 | `interrupcion_terminar` | `x0` llave de interrupción | 0 |
| 12 | `proceso_crear` | `x0` llave de memoria con el ELF, `x1` tamaño del ELF, `x2` tabla de llaves, `x3` cuántas, `x4` llave de grupo o 0 | Llave de proceso |
| 13 | `proceso_matar` | `x0` llave de proceso | 0 |
| 14 | `proceso_esperar` | `x0` llave de proceso | `x0` clase, `x1` código |
| 15 | `llave_duplicar` | `x0` llave, `x1` derechos, `x2` marca | Llave nueva |
| 16 | `llave_revocable` | `x0` llave | `x0` copia revocable, `x1` revocador |
| 17 | `revocar` | `x0` revocador | 0 |
| 18 | `grupo_crear` | `x0` tope en bytes | Llave de grupo |
| 19 | `grupo_matar` | `x0` llave de grupo | 0 |
| 20 | `grupo_uso` | `x0` llave de grupo | `x0` tope, `x1` usado con sus subgrupos, `x2` usado sin ellos |
| 21 | `memoria_quitar` | `x0` dirección donde empieza una proyección | 0 |
| 23 | `registro_leer` | `x0` llave del registro, `x1` búfer, `x2` capacidad | Bytes leídos |
| 22 | `memoria_dma` | `x0` permiso DMA, `x1` tamaño | `x0` llave de memoria, `x1` dirección de bus |
| 25 | `registros_recortar` | `x0` llave de registros o de memoria del firmware, `x1` desde, `x2` tamaño (páginas enteras) | Llave nueva |
| 26 | `dma_liberar` | `x0` permiso DMA | Páginas devueltas |
| 27 | `memoria_tamano` | `x0` llave de memoria (con derecho de leer) | Bytes que mide (páginas enteras) |
| 24 | `temporizador_crear` | `x0` llave de un extremo de canal, `x1` primer plazo en µs, `x2` periodo en µs (0: una vez) | Llave del temporizador |
| 28 | `plazo` | `x0` permiso de plazo (con escribir; no hace falta con capacidad 0), `x1` capacidad en µs (0: vuelve a la clase justa), `x2` periodo en µs (de 1 ms a 1 s) | 0, o `E_LIMITE` si no se admite |
| 29 | `medir` | `x0` 0: núcleo de quien llama; 1, 2, 3: interrupciones, cambios de hilo o µs ociosos del núcleo `x1`; 4: hilos robados entre núcleos; 5, 6, 7: páginas de RAM, páginas libres y bytes del kernel; 8, 9, 10: hilos despertados que pasaron a correr, de ellos tras más de 100 µs, y µs de espera en total; 11: pasadas que dejaron seguir al que corría con uno recién despertado en la cola; 12: páginas del montón del kernel; 13: páginas que no estaban libres al arrancar (imagen del kernel, firmware, árbol); 14: los núcleos en marcha, un bit por núcleo; 15 y 16: bytes libres para procesos ahora y los que había al arrancar (lo que mira el aviso de memoria baja, llamada 53). El 3 cuenta también el rato ocioso de ahora (un núcleo dormido sin tic no parece ocupado) | El número; `E_ARGUMENTO` con un núcleo que no existe |
| 30 | `dormir` | `x0` microsegundos (como mucho una hora) | 0, al despertar (antes si lo matan) |
| 31 | `fijar` | `x0` 1: el hilo que llama no se mueve de su núcleo; 0: el reparto entre núcleos puede moverlo | 0 |
| 32, 33 | — | Reservadas para la ida y vuelta con paso directo (K6) | |
| 34 | `azar` | `x0` búfer, `x1` bytes (hasta 256) | `x0` bytes; `x1` 1 si son de calidad (el kernel tuvo semilla del firmware), 0 si no |
| 35 | `canal_leer_plazo` | Como 5, pero `x4` lleva en sus 16 bits bajos cuántas llaves caben y en el resto el plazo en µs (0: sin plazo) | Como 5, o `E_PLAZO` si vence sin mensaje (K7) |
| 36 | `proceso_avisar` | `x0` proceso (con leer), `x1` canal (con escribir) | 0. Cuando el proceso acabe (o ya, si acabó), el kernel escribe por el canal 16 bytes: clase y código como en `proceso_esperar` (u64 little-endian), con la marca de la llave del canal. Hasta 16 por proceso (`E_LIMITE`); con la cola llena, el aviso se pierde (K7) |
| 37 | `proceso_numero` | `x0` proceso (con leer) | Su número de proceso en el kernel: no se repite, desde 1 (U6) |
| 38 | `grupo_procesos` | `x0` grupo (con leer), `x1` dónde, `x2` cuántos caben (hasta 64) | Cuántos procesos vivos tiene el grupo; escribe sus números (u64) en `x1`. Solo con la llave del grupo (U6) |
| 40 | `hilo_crear` | `x0` entrada, `x1` pila (alineada a 16), `x2` argumento, `x3` TLS | El número del hilo. Baja a EL0 en la entrada con SP = pila, x0 = argumento y TPIDR_EL0 = TLS. `E_ARGUMENTO` si la entrada no es código del proceso o la pila no es memoria suya que se pueda escribir; `E_MEMORIA` si no cabe en el grupo; `E_LIMITE` con 256 hilos sin unir; `E_CERRADO` si el proceso se está acabando |
| 41 | `hilo_unir` | `x0` número de hilo | `x0` clase y `x1` código, como `proceso_esperar`, cuando acaba. Solo hilos de `hilo_crear` del mismo proceso, una vez: si no, `E_ARGUMENTO` |
| 42 | `esperar` | `x0` palabra de 32 bits (alineada a 4), `x1` valor, `x2` plazo en µs (0: sin plazo) | 0 si lo despertaron; 1 si la palabra no valía eso (no duerme); `E_PLAZO`; `E_ARGUMENTO` si la palabra no es memoria normal que pueda leer |
| 43 | `despertar` | `x0` palabra, `x1` cuántos como mucho | Cuántos despertó |
| 44 | `proceso_salir` | `x0` código | No vuelve: acaba el proceso entero, con todos sus hilos |
| 45 | `maquina_crear` | `x0` permiso de máquinas virtuales (con escribir), `x1` llave de memoria (leer y escribir) con la RAM del invitado, `x2` dirección física del invitado donde empieza (alineada a 4 KB), `x3` entrada, `x4` el x0 del invitado | Llave de la vCPU 0 de una máquina virtual nueva (H9). `E_NO_HAY` sin hipervisor |
| 46 | `maquina_correr` | `x0` vCPU (con escribir), `x1` informe (104 bytes), `x2` respuesta a una lectura pendiente, `x3` bloque de interrupciones (80 bytes) o 0 | El motivo de la salida (1 a 7); el informe, en `x1`; el bloque, al día. `E_CERRADO` con la máquina parada o muerta |
| 47 | `maquina_memoria` | `x0` vCPU (con escribir), `x1` llave de memoria (leer y escribir), `x2` dirección física del invitado | 0: el invitado ve también esa memoria (todas sus vCPU, aunque estén corriendo). `E_ARGUMENTO` si pisa otra |
| 48 | `maquina_estado` | `x0` vCPU (con escribir), `x1` operación (0 leer registros, 1 escribirlos, 2 aborto externo, 3 proyectar la GICV, 4 otra vCPU, 5 reiniciarla, 6 patearla, 7 parar la máquina), `x2` dirección de 33 u64 (0 y 1), física del invitado (3), número de vCPU (4) o entrada (5), `x3` entrada (4) o x0 (5), `x4` x0 (4) | 0, la llave de la vCPU nueva (4) o un error: `E_LIMITE` si está corriendo, `E_ARGUMENTO` si no hubo acceso que abortar o el número de vCPU no vale, `E_NO_HAY` sin GIC virtual |
| 50 | `memoria_vista` | `x0` tabla de llaves de memoria (los orígenes, con leer), `x1` cuántas (de 1 a 64), `x2` tabla de números de página (u32), `x3` cuántos (de 1 a 4096) | Llave de un objeto de memoria nuevo hecho con esas páginas de los orígenes, sin copiarlas, solo para leer (H9) |
| 51 | `apagar` | `x0` permiso de apagar (con escribir), `x1` 0 apagar, 1 reiniciar | No vuelve si se pudo. Antes deja 300 ms para que la consola saque lo último del registro. Con PSCI (SYSTEM_OFF, SYSTEM_RESET) si el árbol lo trae (QEMU `virt`, UEFI, la Pi 5 con el BL31 de su firmware; en la Pi 5, que se corte la corriente lo decide su EEPROM, que no se toca); sin PSCI, en la Pi 4 y la Pi 3, con el perro guardián del bloque PM (apagar marca la partición 63 en PM_RSTS). `E_TIPO` si `x0` no es el permiso, `E_ARGUMENTO` con otro modo, `E_NO_HAY` si no se pudo |
| 52 | `cpu_tiempo` | `x0` llave de grupo (con leer), o 0: el grupo propio | Microsegundos de CPU que han gastado los procesos del grupo y de sus subgrupos, vivos y muertos, desde que se creó (las dos clases, también los hilos de una vCPU). `E_LLAVE`, `E_TIPO` (no es un grupo) o `E_DERECHOS` (sin leer). Para el monitor del sistema (trasfondo): solo quien tiene la llave del grupo (`sesiones`, de lo que lanza) mide a otro |
| 53 | `memoria_aviso` | `x0` permiso de memoria (con escribir), `x1` canal (con escribir) | 0. El aviso de memoria baja llega al otro lado del canal (S3). `E_LLAVE`, `E_TIPO` o `E_DERECHOS` |
| 54 | `grupo_tope` | `x0` llave de grupo (con escribir), `x1` tope en bytes | 0. `E_DERECHOS` sin escribir o si es el grupo propio o uno de encima (S3) |
| 55 | `maquina_globo` | `x0` vCPU (con escribir), `x1` 0 soltar, 1 reponer, `x2` tabla de direcciones físicas del invitado (u64, alineadas a 4 KB, en el orden de bytes del proceso), `x3` cuántas (de 1 a 256) | Cuántas páginas se soltaron o repusieron. El globo de la caja Linux (aviso-cajas): soltar quita la página de la etapa 2 y de las proyecciones de quien llama, vacía las TLB y la devuelve al sistema (y sus bytes al grupo del objeto); reponer pone una nueva a cero (cobrada a ese grupo). Solo memoria que no tiene nadie más: las páginas de un objeto que esté en otra máquina, otro proceso, un mensaje, una vista o tras una llave revocable se saltan. `E_ARGUMENTO` si una dirección no es de su RAM o no está alineada (no se toca nada), `E_TAMANO`, `E_MEMORIA` si no se pudo reponer ninguna, `E_NO_HAY` sin hipervisor en el núcleo |
| 49 | `pagina_cero` | `x0` permiso de página 0 (con escribir), `x1` llave de memoria de una página (con leer; no revocable) | 0. La proyecta en la dirección 0, de solo lectura y sin ejecutar (H4). `E_TIPO` si `x0` no es el permiso, `E_TAMANO` si el objeto no mide una página, `E_ARGUMENTO` si ya hay página 0 o la llave es revocable |
| 56 | `puertos_permitir` | `x0` llave de puertos de E/S (solo x86) | 0 |

### Detalles

- **1, `escribir`, y la consola** (C48). Lo escrito va al registro del sistema, un anillo de 64 KB del kernel, y de
  ahí a la consola. Cada línea de un programa empieza por "│ ", así que nunca se confunde con una del kernel. Si el
  anillo está lleno, lo nuevo se pierde. Antes: va a la consola del sistema, que hoy es el puerto serie. El texto se interpreta como UTF-8, y
  cada byte que no lo sea sale como `?`. Los caracteres de control también, salvo el salto de línea y el tabulador.
  Es provisional: la consola será un servicio.
- **4, `canal_escribir`.**
  - Hasta 256 bytes de datos y 8 llaves por mensaje.
  - Las llaves enviadas salen de la tabla de quien escribe y entran en la de quien lee, con los mismos derechos.
  - Cada llave enviada necesita el derecho de transferir.
  - Sin ciclos (C40): no se puede mandar un extremo de este mismo canal (ni el propio ni su pareja), ni una llave de
    interrupción o de temporizador que avise a este canal. Da `E_ARGUMENTO`.
  - La llave por la que se escribe necesita el derecho de escribir.
  - Nunca bloquea: el mensaje se queda en la cola del otro extremo.
  - Una misma llave dos veces en la tabla devuelve `E_ARGUMENTO`, y no se manda nada.
  - Si el otro extremo está cerrado, devuelve `E_CERRADO` y las llaves vuelven a sus números de antes.
- **5, `canal_leer`.**
  - Si la cola está vacía, se bloquea hasta que llegue un mensaje o se cierre el otro extremo.
  - Las llaves recibidas toman números nuevos en la tabla de quien lee.
  - Si el mensaje no cabe en el búfer o en la tabla de llaves, devuelve `E_TAMANO`. Si no se puede escribir donde
    dice el proceso, devuelve `E_ARGUMENTO`. En los dos casos el mensaje vuelve al principio de la cola, con sus
    llaves.
- **3, 4 y 5, tablas de llaves.** Son tablas de números de 32 bits, en el orden de bytes del proceso.
- **6, `cerrar`.** Quita la llave de la tabla. Si era la última llave de un extremo de canal, ese lado se cierra:
  quien esté leyendo al otro lado se despierta con `E_CERRADO` cuando la cola queda vacía, y los mensajes que
  esperaban al lado cerrado se sueltan con sus llaves.
- **Al morir un proceso** (C40) se sueltan todas sus llaves, como si las cerrara una a una: sus extremos de canal se
  cierran y el otro lado se entera. Lo mismo pasa con un extremo que va dentro de un mensaje que ya nadie leerá.
- **7, `memoria_crear`.** Páginas nuevas de 4 KB, todas a cero. La llave tiene todos los derechos. Los últimos
  32 MB libres son del kernel: al llegar ahí devuelve `E_MEMORIA`.
  Ponerlas a cero es lo largo de la llamada: cada 64 páginas en ARM (S2) y tras cada página en x86-64 (X6) abre un
  momento las interrupciones, así que el núcleo atiende el reloj y los aparatos mientras, y el tic puede pasar a otro
  hilo.
- **50, `memoria_vista`** (H9). Una vista: un objeto de memoria hecho con páginas de otros, en el orden que se pide,
  para que el compositor lea sin copia los búferes de un invitado de la caja Linux (están en páginas sueltas de su
  RAM).
  - Los números de página cuentan seguidas las páginas de los orígenes, en el orden de la tabla: la 0 es la primera
    del primero; tras la última del primero viene la primera del segundo. Una página puede salir varias veces. Las dos
    tablas van en el orden de bytes del proceso.
  - La llave nueva tiene leer y transferir, nada más: la vista se proyecta solo para leer (escribir da `E_DERECHOS`;
    ejecutar, `E_ARGUMENTO`, también con otra llave), el kernel no escribe por ella y no vale como RAM de una máquina
    (`maquina_crear` y `maquina_memoria` dan `E_ARGUMENTO`). Lo que se escriba en los orígenes se ve en ella al
    momento.
  - Mantiene vivos sus orígenes: aunque se suelten todas sus llaves y proyecciones, sus páginas no vuelven al sistema
    (ni se dan a otro) hasta que se suelta la vista.
  - Cupo: la vista cobra a quien la pide 256 bytes, más 8 por página y 8 por origen (su lista en el kernel). Las
    páginas siguen cobradas al grupo de quien creó cada origen hasta que se suelte lo último que lo tiene, la vista
    también.
  - Errores: `E_TAMANO` con 0 orígenes o más de 64, o 0 páginas o más de 4096 (16 MB); `E_ARGUMENTO` si una tabla no
    se puede leer, un número de página no está en los orígenes, o un origen es memoria DMA, otra vista o una llave
    revocable (la vista seguiría viendo el objeto después de revocarla); `E_LLAVE`, `E_TIPO` (no es memoria) y
    `E_DERECHOS` (sin leer) por cada origen; `E_MEMORIA` si no cabe en el cupo; `E_LIMITE` con la tabla de llaves
    llena.
- **8, `memoria_proyectar`.**
  - Proyecta el objeto entero a partir de la dirección. Si alguna página del tramo ya tiene algo, no proyecta nada y
    devuelve `E_ARGUMENTO`.
  - Permiso 3 es lectura y escritura, y nunca se ejecuta.
  - Permiso 5 (o 4) es lectura y ejecución, nunca escritura (H4, para un JIT como el de la caja Amiga). Escribir y
    ejecutar por la misma dirección (2 y 4 juntos) da `E_ARGUMENTO`. Un JIT proyecta el mismo objeto dos veces: para
    escribir en una dirección y para ejecutar en otra. Tras escribir código, limpia la caché de datos por la dirección
    de escribir (`dc cvau`) e invalida la de instrucciones por la de ejecutar (`ic ivau`), con `dsb ish` e `isb`.
    En x86-64 no hay cachés que limpiar, pero el código se escribió por otra dirección lineal: antes de ejecutarlo,
    una instrucción que serializa, como `cpuid` (lo hacen `malo` y `relevo`, X7a).
    La memoria de DMA no se ejecuta nunca (`E_ARGUMENTO`).
  - El mismo objeto proyectado en dos procesos es memoria compartida.
  - Sus páginas vuelven al sistema cuando no queda ninguna llave ni ninguna proyección.
- **9, `registros_proyectar`.**
  - Para controladores. La llave de registros solo la da el kernel, al lanzar el controlador.
  - Pide los derechos de leer y escribir.
  - Se proyecta como memoria de dispositivo: sin caché, sin reordenar y nunca se ejecuta.
  - También proyecta una llave de memoria del firmware (C50): la de la VideoCore, donde está la pantalla. Esa va como
    memoria normal sin caché. El kernel comprueba que no toca la RAM que reparte.
  - Los registros de los periféricos de la Pi son little-endian. Un proceso big-endian tiene que darles la vuelta.
- **10, `interrupcion_asociar`.**
  - Solo interrupciones de aparato (SPI, de la 32 a la 255, y con GICv3 las LPI de la 8192 a la 8255, que son las
    MSI que pasan por el ITS), y cada una con un único dueño. Las LPI se enmascaran en el kernel, no en el GIC: una
    que llega enmascarada se avisa al terminar la anterior. El mensaje es el número, 4 bytes.
  - El extremo de canal necesita el derecho de escribir.
  - Cuando llega la interrupción, el kernel la enmascara y escribe en el canal un mensaje de 4 bytes con su número,
    **siempre en little-endian**.
- **11, `interrupcion_terminar`.** La vuelve a habilitar cuando el controlador ya ha atendido el aparato.
- **10 y 11.** Las dos piden el derecho de escribir en la llave de la interrupción.
- **5, `canal_leer`, la marca** (C41). En `x2` devuelve la marca de la llave por la que se escribió el mensaje, o 0.
- **La marca 0** es "sin marca" y también la de lo que escribe el kernel (las interrupciones). Quien recibe una llave
  sin marca puede marcarla como quiera: un servidor que quiera saber quién le escribe solo debe dar llaves marcadas.
- **Varios lectores.** Con copias de una llave, varios hilos pueden leer el mismo lado: cada mensaje lo recibe uno, y
  al cerrarse el otro lado se despiertan todos.
- **15, `llave_duplicar`** (C41). Una copia del mismo objeto con los derechos pedidos que ya tenga la llave, nunca
  más. Con el bit 8 en `x1`, la copia lleva la restricción "una vez"; si la original ya la tenía, la copia también. Si la llave no tenía marca y `x2` no es 0, la copia lleva esa marca; con `x2` a 0 conserva la suya. Marcar con
  otra una llave ya marcada da `E_DERECHOS`. Un lado de canal sigue abierto mientras quede alguna copia.
- **16, `llave_revocable`.** Una copia que pasa por un intermediario, y el revocador de ese intermediario. Las copias
  que se hagan de la revocable también pasan por él. Cada intermediario cuesta 256 bytes del cupo. Como mucho hay 4
  intermediarios uno dentro de otro, contando los de los revocadores; el quinto da `E_LIMITE`.
- **Grupos y cupos** (C42; topes desde S3). Cada proceso es de un grupo con un tope en bytes. Todo lo que crea se cobra
  ahí y en todos los grupos de encima, hasta el raíz:
  - un objeto de memoria, por sus páginas;
  - su espacio, por sus tablas y sus páginas de imagen y pila;
  - cada proceso, unos 64 KB fijos: su tabla de llaves llena (41 KB; no crece más), la pila del kernel (16) y el hilo
    con su estado de coma flotante, el proceso y lo que redondea el montón (8);
  - cada canal, unos 26 KB fijos (sus dos colas llenas).

  Si algún grupo de la cadena pasaría de su tope, no se cobra nada y la llamada da `E_MEMORIA`; también si no queda
  memoria física. El tope no aparta nada: los topes de los hijos pueden sumar más que el del padre y más que la RAM, y
  lo que no se pasa nunca es lo usado de verdad. Lo cobrado vuelve, en toda la cadena, cuando se suelta el objeto, aunque
  lo tenga otro proceso. Los procesos que lanza el kernel son del grupo raíz, cuyo tope es la memoria libre para
  procesos al arrancar, menos la reserva del kernel. Las sumas son atómicas y sin cerrojos: dos cobros a la vez no pasan
  de un tope; mientras uno que no cabe deshace lo sumado abajo, otro puede fallar por un momento.
- **18, `grupo_crear`.** Un subgrupo del propio con ese tope. Del propio solo se cobran los 512 bytes del grupo en sí
  (si no caben, `E_MEMORIA`), que vuelven cuando ya no queda nada cobrado al subgrupo. Un tope mayor que el del propio
  vale, pero no sirve de nada: el del propio para antes. Como mucho hay 7 niveles bajo el raíz. Un grupo matado no admite
  procesos ni subgrupos nuevos.
- **12 con `x4`.** Con una llave de grupo con derecho de escribir, el proceso nuevo es de ese grupo; con 0, del de
  quien lo crea.
- **19, `grupo_matar`.** Pide el derecho de escribir. Mata a todos los procesos del grupo y de sus subgrupos, y cierra
  los dos lados de todos los canales que crearon (C44), aunque tengan llaves en otros grupos: quien los use después
  recibe `E_CERRADO`. Así se suelta todo lo que el grupo dejó, también los ciclos entre canales.
- **20, `grupo_uso`.** Pide el derecho de leer. Devuelve el tope (`x0`), lo usado por el grupo con sus subgrupos (`x1`)
  y lo usado sin ellos (`x2`, lo suyo menos lo de cada subgrupo vivo; una foto que, con cobros a la vez, puede ir algo
  por delante o por detrás). En la biblioteca, `grupo_uso` y `grupo_uso_propio`.
- **54, `grupo_tope`** (S3). Cambia el tope de un grupo. Pide su llave con escribir, y que no sea el grupo de quien
  llama ni uno de encima: un proceso no se sube su tope (la caja Amiga no cambia el suyo; se lo pone `sesiones`, que
  tiene la llave de su grupo). Uno por debajo de lo usado vale: no se quita nada, y todo cobro falla hasta que lo usado
  baje de él. Lo prueba `grupos` (pasos 36 a 39).
- **53, `memoria_aviso`** (S3). El aviso de memoria baja. Pide el permiso de memoria (llave de arranque del tipo 11,
  "memoria", que `inicio` manda solo a `sesiones`) y un canal con escribir; los avisos van al otro lado, con la marca de esa llave, y dejan de
  llegar si se revoca. Hay un solo oyente: uno nuevo quita al anterior. Nada más pedirlo llega el nivel de ahora, y
  después cada cambio: memoria baja cuando lo libre para procesos baja del 5 % de lo que había al arrancar, y normal
  cuando vuelve a pasar del 10 %. Se mira al acabar cada llamada al sistema. El mensaje, de 24 bytes en little-endian:
  el nivel (u32: 0 normal, 1 baja), 0 (u32), los bytes libres para procesos (u64) y los que había al arrancar (u64). Con
  la cola llena, ese aviso se pierde. Lo prueba `prueba_topes` en `make prueba`.
- **55, `maquina_globo`** (aviso-cajas, `docs/bitacoras/aviso-cajas.md`). El globo (virtio-balloon) de la caja Linux.
  Con la llave de una vCPU (escribir) y una tabla de direcciones físicas del invitado (u64, alineadas a 4 KB, de su RAM;
  de 1 a 256), suelta o repone esas páginas. Puede ser con sus vCPU en marcha.
  - Soltar: la página sale de la etapa 2 y de las proyecciones del objeto en el proceso que llama (el servicio ve la
    RAM del invitado), se vacían las TLB de ese VMID en todos los núcleos y la página vuelve al sistema; sus bytes se
    devuelven al grupo que creó el objeto. Desde ahí, si el invitado la toca sale al servicio como un aparato que no
    existe, y si la toca el servicio, es un fallo suyo (el de la caja no la toca: lleva la cuenta).
  - Reponer: una página nueva a cero (limpia hasta la RAM: el invitado puede leerla sin caché) en su sitio, en la
    etapa 2 y en esas proyecciones, cobrada al grupo del objeto. `E_MEMORIA` si no se pudo ninguna.
  - Solo memoria que no tiene nadie más: el kernel cuenta las referencias del objeto y tienen que ser las de esta máquina,
    las proyecciones y las llaves (sin intermediario) de quien llama, con su tabla de llaves y su espacio tomados. Las
    páginas de un objeto que esté en otra máquina, en otro proceso, en un mensaje sin leer, en una vista o tras una llave
    revocable se saltan. Una página soltada no está en ninguna vista nueva (`memoria_vista` da `E_ARGUMENTO`), no se
    proyecta (queda un hueco) y el kernel no lee ni escribe por ella.
  - `E_ARGUMENTO` si alguna dirección no está alineada o no es de su RAM (no se toca nada), `E_TAMANO` con 0 o más de
    256, `E_TIPO`, `E_DERECHOS`, `E_NO_HAY` sin hipervisor en el núcleo. Lo prueba `caja-linux` (`prueba_globo`) en
    `make arranque-virt-vm` y `-linux`.
- **52, `cpu_tiempo`** (trasfondo). El planificador suma lo que corre cada hilo, en cada cambio, a su grupo y a todos los
  de encima (sumas atómicas, sin cerrojos ni memoria). Lo del hilo que corre ahora entra en su siguiente cambio (como
  mucho un tic tarde). El contador no baja nunca, ni al matar el grupo. Lo prueba `grupos` en `make prueba`: un hijo
  que gira cuenta en su grupo y en el de arriba, no se pierde al matarlo, y sin leer, con otra llave o con una que no
  existe, no se lee.
- **17, `revocar`.** Pide el derecho de escribir en el revocador. Desde ese momento, la copia revocable y las suyas
  dan `E_LLAVE`, estén donde estén. El objeto se suelta: si era lo último que tenía un lado de canal, el lado se
  cierra. Si es memoria, se quitan las proyecciones que se hicieron con la copia revocable o con sus copias, en todos
  los procesos (C45); las hechas con la llave original siguen.
- **56, `puertos_permitir`** (X6, solo x86-64; era la 29 hasta la fusión con `revision`, donde la 29 es `medir`). Con una llave de puertos de E/S (leer y escribir), el proceso puede
  usar esos puertos con `in` y `out` desde el anillo 3; los demás siguen terminándolo. Va al mapa de E/S de la TSS
  de cada núcleo, que se pone al cambiar a un hilo del proceso: vale ya en el hilo que llama, y en los demás hilos del
  proceso desde su próximo cambio. Solo puertos por debajo de 0x400 (`E_ARGUMENTO` si no). Solo el kernel crea estas
  llaves: la de COM1 (0x3F8 a 0x3FF), para la consola. En ARM no hay llaves de puertos: siempre `E_TIPO`.
- **23, `registro_leer`** (C48). Para la consola: pide la llave del registro, que solo da el kernel a `inicio`. Saca
  hasta 4096 bytes del registro, esperando si no hay nada. La primera vez, el kernel deja el puerto serie: desde ahí
  solo escribe en el registro, salvo el pánico, que va directo.
- **22, `memoria_dma`** (C46). Pide una llave de permiso DMA con derecho de escribir; solo la da el kernel, a los
  controladores que la necesitan. Da un objeto de memoria de hasta 4 MB de páginas seguidas de la zona del permiso
  (el primer GB, el que alcanza el DMA; la de la GPU, de la zona normal), a cero, cobrado al cupo, y en `x1` la
  dirección de bus de su primera página, la que hay que dar al aparato. Se proyecta sin caché, también para leer: el
  aparato y el proceso ven lo mismo sin limpiar cachés. Al soltarse, va a la cuarentena del permiso (ver 26). Con otra
  llave, `E_TIPO`; con más de 4 MB, `E_TAMANO`.
- **21, `memoria_quitar`** (C45). Quita la proyección de un objeto de memoria que empieza en esa dirección: el tramo
  entero que se proyectó. Desde ese momento, leer o escribir ahí es un fallo. Si no empieza ninguna ahí,
  `E_ARGUMENTO`. Los registros de aparatos no se quitan así.
- **24, `temporizador_crear`** (C49). Pasado el primer plazo, y después cada periodo si no es 0, llega un mensaje al
  otro lado del canal: 8 bytes, un número de 64 bits en little-endian con las veces que ha vencido desde el último
  aviso que se pudo entregar (casi siempre 1; más si la cola del canal estaba llena o el aviso llegó tarde). Así se
  distingue de una interrupción, que llega en 4 bytes. Pide el derecho de escribir en el canal (`E_DERECHOS`); otra
  llave que no sea de canal da `E_TIPO`; un plazo o un periodo de más de un día, `E_ARGUMENTO`. Cuesta 4 KB del cupo
  del grupo. Resolución: la del contador de la CPU (M2); el periodo más corto, 100 µs. Nunca avisa antes de tiempo.
  Los avisos llevan la marca de la llave del canal con que se creó (C41); si esa llave era revocable y se revoca, el
  temporizador se para para siempre. Si la cola está llena, el aviso se cuenta para el siguiente; uno de una vez se
  reintenta en cada tic hasta que cabe. Se para al soltarse su última llave (`cerrar`, o al morir el proceso). Para
  esperar un rato sin gastar CPU: un temporizador de una vez y leer el canal. Lo mismo vale para `interrupcion_asociar`:
  sus avisos llevan la marca de la llave del canal y dejan de llegar si se revoca.
- **25, `registros_recortar`** (auditoría de la fase 4, M4). Una llave nueva del mismo tipo que solo cubre
  `[desde, desde + tamaño)` del tramo de la original, con sus mismos derechos. Los dos, múltiplos de 4 KB; fuera del
  tramo o tamaño 0, `E_ARGUMENTO`; otra llave, `E_TIPO`; una llave revocable, `E_ARGUMENTO` (la copia no se podría
  revocar). Así quien tiene un tramo grande da solo un trozo.
- **26, `dma_liberar`** (M5). La memoria DMA, al soltarse, no vuelve al sistema: queda en la cuarentena de su
  permiso, porque el aparato puede seguir escribiendo en ella (su controlador murió a medias). El controlador nuevo,
  con el aparato ya reiniciado, la devuelve con esta llamada. Mientras, sigue cobrada al grupo que la pidió. Pide el
  derecho de escribir en el permiso.
- **12, `proceso_crear`** (C39).
  - El ELF son los primeros `tamaño` bytes del objeto de memoria, que necesita el derecho de leer. Se copia al
    crear: después se puede cambiar o soltar el objeto.
  - Las llaves de la tabla (hasta 8, con derecho de transferir y sin repetir) salen de la tabla de quien crea y son
    las llaves 1, 2... del proceso nuevo. Si no se puede crear, vuelven a sus números.
  - El kernel elige el núcleo. La llave de proceso tiene todos los derechos.
  - Errores: `E_ARGUMENTO` si el ELF no vale, `E_TAMANO` si el tamaño es 0 o mayor que el objeto, `E_MEMORIA`,
    `E_LIMITE` si no cabe la llave nueva.
- **13, `proceso_matar`.** Pide el derecho de escribir. Vale para un proceso que corre, que espera o que ya acabó.
  Cada hilo muere en su siguiente vuelta a EL0 o al despertar si estaba bloqueado; la llamada no espera a eso.
- **14, `proceso_esperar`.** Pide el derecho de leer. Se bloquea hasta que acabe el último hilo del proceso.
  - Clase 0: salió con `salir`, y `x1` es el código.
  - Clase 1: lo mataron.
  - Clase 2: murió por un fallo.
  - Si ya había acabado, contesta al momento, todas las veces que se pregunte.
  - La llave de proceso no mantiene viva la memoria del proceso: se libera cuando acaba.

- **45 a 48, máquinas virtuales** (caja Linux, H9; `kernel/src/maquinas.rs`, `base/hiper.rs`). Solo si el kernel
  arrancó en EL2 (en QEMU, `virtualization=on`); si no, `maquina_crear` da `E_NO_HAY`. En x86 (X10), con AMD-V o VT-x:
  lo que cambia está en la sección "x86-64". El kernel deja en EL2 un
  trozo pequeño que cambia de mundo, y sigue en EL1.
  - Cada llave de máquina es una vCPU. Las de una máquina comparten su memoria, la etapa 2 y el VMID; cada una tiene
    su MPIDR, sus registros, su GIC virtual (LR, VMCR, APR), su temporizador virtual y su FPU, y se corre desde un
    hilo, a la vez que las demás en otros núcleos. Si en un núcleo corre una vCPU distinta de la última que corrió ahí
    de la misma máquina, EL2 vacía antes lo de ese VMID de la TLB de ese núcleo.
  - `maquina_crear`: la vCPU 0 de una máquina nueva, en EL1, con la MMU y las interrupciones apagadas, que empieza en
    `x3` con `x4` en su x0 (el árbol de dispositivos de un kernel de Linux) y ve el objeto de memoria entero desde `x2`
    (hasta 4 GB de direcciones del invitado). Se ve como el núcleo 0 (MPIDR 0x80000000). Con `maquina_memoria` ve más objetos, cada
    uno desde su dirección (así pasa de los 16 MB de `memoria_crear`). La memoria se sigue pudiendo proyectar en el
    proceso (así se carga el invitado), pero el invitado la lee sin caché mientras tenga la MMU apagada: lo escrito
    después de crear la máquina puede no verlo. Ni memoria DMA ni una llave revocable (`E_ARGUMENTO`). Su vCPU y sus
    tablas de la etapa 2 se cobran al grupo (unas 4 páginas para 64 KB). Como mucho 255 máquinas a la vez
    (`E_LIMITE`). Se para y se suelta al cerrar su última llave (la de su última vCPU).
  - `maquina_correr`: entra en el invitado hasta que sale. El informe son 13 u64 en little-endian: motivo, PC,
    dirección, valor, tamaño, ESR, x1, x2, las cuentas del contador que pasó fuera del kernel, x3, el plazo del
    temporizador virtual del invitado (CNTV_CVAL si está encendido y sin enmascarar; si no, todo unos), señales
    (bit 0: hay una interrupción pendiente en sus registros de lista: un WFI no tiene que esperar) y los µs que durmió
    en el kernel en sus WFI. Motivos:
    1 HVC (valor = x0, tamaño = el número del HVC, y x1, x2 y x3; el PC ya está en la siguiente); 2 lectura en una dirección que no es su memoria
    (dirección y tamaño; el `x2` de la siguiente llamada es lo que lee); 3 escritura ahí (dirección, valor y tamaño);
    4 WFI; 5 lo sacó una interrupción del anfitrión (el reloj lo hace cada tic: un invitado no se queda con el
    núcleo) o una patada; 6 una trampa o un fallo (ESR, PC y dirección): SMC, el temporizador físico, la depuración, la coma
    flotante, un acceso que no se describe solo. Tras un 6 la máquina entera está muerta (todas sus vCPU salen) y
    `maquina_correr` da `E_CERRADO`.
    7: tocó un registro del sistema atrapado (depuración, contadores de rendimiento, el temporizador físico): dirección
    = su codificación (los bits de op0, op2, op1, CRn y CRm del ESR), x1 = 1 si lee (el `x2` de la siguiente llamada
    es lo que lee) o 0 si escribe (valor). Lo escrito nunca llega al registro de verdad.
    Si otro hilo la está corriendo, `E_LIMITE`. En 2 y 3 el PC ya está en la instrucción siguiente.
    El bloque de interrupciones (10 u64, con un GICv2 con extensiones de virtualización: el GIC-400 de la Pi 4, QEMU
    `virt` con `virtualization=on`): [0] cuántos registros de lista llena el servicio (lo pone el kernel: todos menos
    el último, que es del temporizador virtual; 0 sin GIC virtual), [1] cómo tiene el invitado la PPI 27 en su
    distribuidor (lo pone el servicio: bit 0 habilitada, [15:8] prioridad, bit 16 grupo 1; bit 17, `ESPERAR_WFI`: un
    WFI sin nada pendiente duerme el hilo en el kernel hasta el plazo del temporizador del invitado, y entonces vuelve
    a entrar sin pasar por el servicio, hasta una patada o como mucho 100 ms) y desde [2] hasta 8
    registros de lista (GICH_LRn): los pone el servicio y vuelven como los dejó el invitado. El kernel quita el bit
    HW y los bits reservados, y deja a 0 uno sin estado. El temporizador virtual lo inyecta el kernel en su LR, con
    HW y la PPI 27 física activa mientras está ahí: el EOI del invitado la desactiva. Con un LR con el bit EOI, la
    interrupción de mantenimiento (PPI 25) saca al invitado cuando la termina.
    La primera vez que el invitado usa la FPU sale por la trampa de CPTR_EL2, y desde ahí la vCPU tiene sus registros
    de FP/SIMD, que se cargan y guardan en cada vuelta (los del hilo dueño de la FPU se guardan antes).
  - `maquina_estado`: con la vCPU parada. 0 y 1 leen y escriben x0..x30, PC y PSTATE (de PSTATE, NZCV, DAIF y el
    modo, que tiene que ser EL0t, EL1t o EL1h de AArch64; si no, EL1h). 2 cambia el último acceso a un aparato por un
    aborto externo síncrono de datos en el vector de EL1 del invitado (ESR_EL1 con DFSC 0x10, FAR_EL1, ELR_EL1,
    SPSR_EL1). 3 proyecta la GICV (8 KB, como aparato) en esa dirección del invitado, sin pisar su memoria (una vez
    para todas sus vCPU: cada núcleo ve ahí su interfaz virtual). 4 da otra vCPU de la misma máquina: número `x2`
    (su Aff0, menos de 8 y sin otra con ese número), que empieza en `x3` con `x4` en su x0. 5 deja la vCPU parada como
    recién encendida (PSCI CPU_ON): en `x2`, con `x3` en x0, EL1h con DAIF enmascarado, la MMU apagada, los registros
    de EL1, el temporizador y el GIC virtual a cero y sin FPU propia. 6 la patea: si corre en otro núcleo, una SGI
    (la 2) a ese núcleo la saca; si duerme en un WFI, la despierta; si no, la próxima vez vuelve sin entrar (motivo 5).
    7 para la máquina entera: todas sus vCPU salen y ya no entran (`E_CERRADO`). 6 y 7, desde cualquier hilo y con la
    vCPU corriendo.

### Errores

| Valor | Nombre | Cuándo |
|---|---|---|
| −1 | `E_ARGUMENTO` | Dirección fuera del espacio de usuario o mal alineada; o la llamada viene de un hilo sin proceso |
| −2 | `E_LLAVE` | La llave no existe en la tabla |
| −3 | `E_DERECHOS` | Falta un derecho: leer, escribir o transferir |
| −4 | `E_CERRADO` | El otro extremo del canal está cerrado |
| −5 | `E_TAMANO` | Demasiados datos o llaves, tamaño 0 o más de 16 MB (4 MB en `memoria_dma`) |
| −6 | `E_TIPO` | La llave es de otro tipo de objeto |
| −7 | `E_MEMORIA` | No queda memoria para procesos |
| −8 | `E_LIMITE` | Un límite: tabla de llaves llena, cola del canal llena o demasiadas proyecciones |
| −9 | `E_PLAZO` | Venció el plazo de `canal_leer_plazo` sin mensaje (K7). `canal_leer` ya no dice `E_CERRADO` por falta de memoria: `E_MEMORIA` |
| −10 | `E_NO_HAY` | Sin hipervisor: el kernel no arrancó en EL2, o en x86 no hay AMD-V con NPT ni VT-x con EPT (`maquina_crear`, `maquina_correr`) |

### Topes de trabajo

Una llamada corre entera sin que el kernel se interrumpa, así que cada una tiene un tope (desde C35):
`memoria_crear` pide como mucho 16 MB (con puntos de corte cada 256 KB) y `escribir` toma como mucho 256 bytes. Para más, se llama varias veces.

### Límites

Son límites de forma, aparte del cupo de cada grupo.

| Qué | Límite |
|---|---|
| Llaves en la tabla de un proceso | 1024 |
| Mensajes esperando en un lado de un canal | 16 |
| Proyecciones de objetos de memoria por proceso | 1024 |
| Hilos de `hilo_crear` vivos o sin unir por proceso | 256 |
| Memoria libre que solo usa el kernel | 32 MB |

Con la cola llena, `canal_escribir` devuelve `E_LIMITE` y las llaves vuelven a sus números. Una interrupción que
llega con la cola llena se pierde.

Un número de llamada desconocido devuelve `E_ARGUMENTO` y se avisa en la consola. La 99 solo existe en la imagen de pruebas (opción `pruebas`).

## Llaves y objetos

Una llave es un número de la tabla del proceso. El 0 no es nunca una llave válida. Cada llave apunta a un objeto y
lleva unos derechos:

| Derecho | Bit |
|---|---|
| Leer | 1 |
| Escribir | 2 |
| Transferir | 4 |
| Una vez (restricción) | 8 |

**Una vez** (S4 de `REVISION_ARQUITECTURA.md`) no es un derecho sino una restricción: la llave se puede transferir,
pero llega a su nuevo dueño (por `canal_leer` o al nacer con `proceso_crear`) sin el derecho de transferir y sin esta
marca. Al duplicar se hereda siempre: quien la tiene no se la puede quitar. `inicio` la pone en las llaves de aparato.

Un proceso nuevo no tiene ninguna llave. Solo tiene las que le da quien lo lanza, y esas ocupan los números 1, 2,
etc. El kernel solo lanza `inicio`, con las llaves de arranque; los controladores y demás servicios reciben las suyas
de `inicio` (ver "El programa de inicio" y `servicios.md`).

Tipos de objeto:

| Objeto | Se crea con | Para |
|---|---|---|
| Extremo de canal | `canal_crear` | Mensajes y paso de llaves |
| Memoria | `memoria_crear`; `memoria_vista` (una vista: páginas de otras, solo para leer) | Proyectar y compartir |
| Registros | Solo el kernel | Controladores fuera del kernel |
| Memoria del firmware | Solo el kernel | La de la VideoCore, para el buzón, que da al compositor solo la pantalla (C50) |
| Interrupción | Solo el kernel | Controladores fuera del kernel |
| Proceso | `proceso_crear` | Matarlo (escribir) y esperar a que acabe (leer) |
| Revocador | `llave_revocable` | Revocar la copia (escribir) |
| Permiso DMA | Solo el kernel | Crear memoria DMA (escribir) |
| Registro | Solo el kernel | Sacar el registro del sistema (leer) |
| Puertos de E/S | Solo el kernel, en x86 | Usarlos desde el proceso (`puertos_permitir`); la consola, COM1 (X6) |
| Permiso de plazo | Solo el kernel (`inicio` lo reparte, S3) | Pedir la clase de plazo (escribir) |
| Permiso de página 0 | Solo el kernel (`inicio` lo da a la caja Amiga, H4) | Proyectar la página 0 de solo lectura (escribir; llamada 49) |
| Grupo | `grupo_crear` | Crear procesos en él y matarlo (escribir), ver su uso (leer) |
| Temporizador | `temporizador_crear` | Avisos por un canal pasado un plazo; se para al cerrarlo |
| Permiso de máquinas virtuales | Solo el kernel (`inicio` lo da a `caja-linux`) | Crear máquinas virtuales (escribir) |
| Permiso de apagar | Solo el kernel (`inicio` lo manda a `sesiones`, sin derecho de transferirlo) | Apagar y reiniciar la máquina (escribir; llamada 51) |
| vCPU de una máquina virtual | `maquina_crear` (la 0), `maquina_estado` 4 (las demás) | Correrla, darle más memoria a su máquina, patearla, pararla (escribir); la máquina se para al cerrar su última vCPU |

## Biblioteca de usuario en Rust

`usuario/src/lib.rs` (crate `dashos`) envuelve las llamadas: `escribir`, `salir`, `canal_crear`, `canal_escribir`,
`canal_leer`, `memoria_crear`, `memoria_proyectar`, `registros_proyectar`, `interrupcion_asociar` e
`interrupcion_terminar`. `dashos::hilos` tiene las de hilos (`crear`, `unir`, `esperar`, `despertar`,
`proceso_salir`) y un `Cerrojo` hecho con la espera. También tiene `println!` y la macro `programa!(principal)`, que pone `_start` y el
manejador de pánico.

Los objetivos de compilación son dos:

- `aarch64-unknown-none-softfloat`: el nativo, que se usa por defecto.
- `aarch64_be-unknown-none-softfloat`: para la caja Amiga.

Los programas Dash para DashOS se compilan con `./dash --dashos` en la rama `aarch64` de FreeMEMLang.

### Registros que ve un proceso

- `TPIDR_EL0` es de cada hilo: empieza a 0 (o al TLS de `hilo_crear`) y el kernel lo guarda y lo repone. `TPIDRRO_EL0` vale 0.
- Puede leer el contador virtual (`CNTVCT_EL0`) y su frecuencia. El temporizador físico, los demás temporizadores y
  los contadores de rendimiento terminan el proceso.
- Saltar a una dirección del kernel termina el proceso.
- Las FIQ van enmascaradas. Un SError mientras corre el proceso lo termina.

## x86-64 (S30, X4 a X6, X5b y X10)

En x86-64 (rama `x86`, `make -C kernel prueba-x86` y `make -C kernel arranque-x86`) un programa ve lo mismo que en ARM: las mismas llamadas con los
mismos números, argumentos, resultados y errores, las mismas llaves y objetos, el mismo mapa de direcciones y los
mismos límites. Cambian los registros y lo que es de la CPU:

- **Llamadas.** Se entra con `syscall`. El número va en `rax` y los argumentos en `rdi`, `rsi`, `rdx`, `r10` y `r8`
  (los de ARM `x0` a `x4`, en ese orden; `r10` en vez de `rcx`, que `syscall` usa para la vuelta). El resultado vuelve
  en `rax` (`x0` en ARM) y, en las llamadas que dan más, en `rdx` (`x1`) y `r8` (`x2`, la marca de `canal_leer`).
  `syscall` pisa `rcx` y `r11`; todos los demás registros vuelven como estaban. La llamada corre con las interrupciones
  cerradas, como en ARM (salvo los puntos de corte de las llamadas largas: S2, X6), y vuelve con `sysret`.
- **Ejecutable.** ELF64 de x86-64 (`e_machine` 62), solo little-endian: x86 no tiene big-endian. Lo demás, igual:
  `ET_EXEC` o `ET_DYN`, segmentos, pila y reglas de rechazo. Un PIE aplica él mismo sus `R_X86_64_RELATIVE`.
- **Al arrancar.** `rsp` apunta al tope de la pila (alineado a 16, como pide el System V al entrar en un proceso) y
  `rip` a la entrada. `rdi` lleva la frecuencia del contador en Hz (lo que en ARM da `CNTFRQ_EL0`). Los demás
  registros valen cero y RFLAGS 0x202 (interrupciones abiertas).
- **Hilos de usuario** (llamadas 40 a 44, desde la fusión con `revision`). Como en ARM, con estas diferencias:
  la entrada no tiene que estar alineada; el hilo empieza con `rdi` = argumento, la base de FS = TLS y `rsp` = pila
  menos 8 (como tras un `call`: la entrada puede ser una función de C); y cuesta 24 KB al grupo, no 20 (su zona de
  XSAVE). La prueba es la misma (`hilos`, C52 en `make -C kernel prueba-x86`).
- **Lo que no hay todavía en x86.** El globo de las máquinas virtuales (55) da `E_NO_HAY`, y las llaves de arranque
  no traen el permiso de máquinas (`inicio_x86.rs` no lanza la caja Linux). `apagar` (51) reinicia (puerto 0xCF9 y, si no,
  el controlador del teclado), pero no apaga: da `E_NO_HAY` (haría falta leer `\_S5` del AML de ACPI). `hora` (39) da
  0: no se lee el reloj del PC. `azar` (34) dice siempre que no es de calidad: no hay semilla del firmware (falta
  RDRAND o RDSEED). No hay orden de bytes por proceso: todo es little-endian. Las llaves de arranque no traen los
  permisos de página 0, de apagar ni del aviso de memoria (`inicio_x86.rs` no lanza `sesiones` ni la caja Amiga);
  las llamadas 49, 53 y 54 sí están.
- **Máquinas virtuales** (X10; `kernel/src/maquinas_x86.rs`, `base/x86_64/hiper.rs`). Las llamadas 45 a 48, con los
  mismos números, llaves, informe (13 u64) y bloque de interrupciones (10 u64) que en ARM, con AMD-V (SVM) y NPT o con
  VT-x y EPT (con invitado sin restricciones). Sin ninguno de los dos, `maquina_crear` da `E_NO_HAY`. El kernel dice
  cuál hay al arrancar ("Hipervisor: ..."). Lo que cambia:
  - **La vCPU empieza** en modo protegido de 32 bits plano, como pide el protocolo de 32 bits de Linux: CS 0x10 y los
    de datos 0x18, de base 0 y límite 4 GB; sin paginación; CR0 = 0x31 (PE, ET, NE); EFER, CR3, CR4 a 0; RFLAGS 0x2
    (interrupciones cerradas); GDTR e IDTR de base 0 y límite 0xFFFF (el invitado pone las suyas); rip = `x3` y
    **rsi = `x4`** (el "x0" de ARM). XCR0 = 1 y la FPU como al encender. Cada vCPU tiene su ID de APIC (su número, de
    0 a 7) en CPUID.
  - **La memoria** del invitado, hasta 512 GB de direcciones físicas (en ARM, 4 GB). Sin cambios de caché que hacer:
    x86 es coherente.
  - **Motivos.** 1 HVC es VMMCALL (AMD) o VMCALL (Intel): valor = rax, x1, x2 y x3 = rbx, rcx y rdx, tamaño 0. 2 y 3,
    lectura o escritura en una dirección que no es su memoria: el kernel decodifica la instrucción (la lee de la
    memoria del invitado recorriendo sus tablas de páginas, o la da la CPU en AMD con su ayuda): MOV entre registro y
    memoria, MOV de un inmediato, MOV con dirección absoluta (A0 a A3), MOVZX y MOVSX; de 1, 2, 4 u 8 bytes, con
    prefijos y REX, también a AH, CH, DH y BH. Otra instrucción es un fallo (6). La respuesta a una lectura se escribe
    como lo haría la CPU (4 bytes: el resto del registro a cero; 1 o 2: el resto como estaba). 4 es HLT. 5, una
    interrupción del anfitrión o una patada. 6, triple fallo, estado no válido, un fallo de NPT/EPT que no es de un
    acceso a un aparato (ejecutar ahí, una tabla del invitado fuera de su memoria), INS/OUTS o una instrucción que no
    se sabe decodificar: la máquina queda muerta, como en ARM. 7, REGISTRO, es RDMSR o WRMSR de una MSR que no
    resuelve el kernel: dirección = la MSR, x1 = 1 si lee (el `x2` de la siguiente llamada va a edx:eax), valor =
    edx:eax si escribe. **8, PUERTO** (nuevo, solo x86): IN u OUT; dirección = el puerto, tamaño 1, 2 o 4, x1 = 1 si
    es IN (el `x2` de la siguiente llamada va a al, ax o eax) y valor si es OUT. En todos menos 2, 3 y 6, el RIP ya
    está en la siguiente instrucción; en 2 y 3 también, tras decodificar. La palabra 5 del informe (el ESR en ARM) es
    el código de salida de la CPU (el de AMD o la razón de Intel), para depurar.
  - **Lo que resuelve el kernel sin salir:** CPUID (el de la CPU sin VMX, SVM, MONITOR, x2APIC, PKU, los contadores,
    RDTSCP ni RDPID, y con el bit de hipervisor, el plazo del TSC y la hoja 0x4000_0000 = "DashOSDashOS"); EFER, PAT y
    IA32_TSC_DEADLINE; las MSR que cambian de mundo con el invitado (bases de FS y GS, KernelGsBase, STAR, LSTAR,
    CSTAR, SFMASK, SYSENTER) sin salir; leer el TSC (el del anfitrión, sin desplazamiento); XSETBV (lo que cabe en lo
    que guarda el kernel: x87, SSE, AVX y AVX-512); en VT-x, CR0, CR4 y CR8 (el invitado ve sus valores). Las
    instrucciones de virtualización, MONITOR y MWAIT dan #UD al invitado; RDPMC, #GP; #AC se le devuelve.
  - **El bloque de interrupciones.** [0] = 8 (los huecos que usa el servicio; lo pone el kernel). [1], el
    temporizador del invitado, que lleva el kernel como el virtual de ARM: bit 0 encendido y [15:8] su vector (lo pone
    el servicio, que emula la LVT del APIC), bit 17 `ESPERAR_WFI` (igual que en ARM, con HLT), y el kernel pone el
    bit 18 (`TEMPORIZADOR_ENTREGADO`) si lo entregó en esa llamada. El plazo es el que escribe el invitado en
    IA32_TSC_DEADLINE (en cuentas del TSC, las del contador del anfitrión; la palabra 10 del informe); al vencer, el
    kernel adelanta el tic del núcleo para sacar al invitado a tiempo, lo entrega y lo pone a 0. Desde [2], 8 huecos:
    el vector (bits 7:0, de 16 en adelante) y el bit 8 si está pendiente. El kernel entrega el mayor de los pendientes
    (y del temporizador) en cuanto el invitado lo acepta (IF a 1, sin el bloqueo de STI) y deja ese hueco a 0; uno a
    la vez (el siguiente, en otra salida: el EOI del invitado al APIC es una escritura que sale). El APIC local, el
    IOAPIC y el PIC los emula el servicio. Un HLT con algo que entregar y las interrupciones abiertas no sale: se
    entrega. Señales (palabra 11), bit 0: hay algo pendiente que entregar.
  - **`maquina_estado`.** 0 y 1, los registros: 33 u64 como en ARM, pero rax, rcx, rdx, rbx, rsp, rbp, rsi, rdi, r8 a
    r15, rip, rflags, cr0, cr2, cr3, cr4 y EFER (y 10 a cero). Al escribir, solo los 16 generales, rip y rflags (sin
    VM ni bits reservados; el bit 1 a 1). 2, "no hay nada ahí", en lugar del último acceso: una lectura de un aparato
    o de un puerto da todo unos y una escritura se pierde (como en un PC); en una MSR, #GP en esa instrucción. 3 (la
    GICV) da `E_NO_HAY`. 4, otra vCPU (su ID de APIC). 5, reiniciarla, con un argumento más: `x4` es el modo, 0
    protegido de 32 bits (como al crearla) o 1 real, como tras una SIPI (CS = `x2` / 16, IP = `x2` % 16; para la SIPI
    del vector V, `x2` = V × 4096). 6 y 7, igual.
  - **Para pasar aparatos (X11)**: la dirección física del invitado de cada página será la misma que vea el aparato, y
    el kernel ya sabe dar cada par (física del invitado, física de verdad) para el dominio de la IOMMU
    (`hiper::Espacio::por_cada_pagina`).
  - **Lo que falta.** El globo (55: `E_NO_HAY`). APIC virtual por hardware (AVIC, APICv) e interrupciones entregadas
    sin salir (posted interrupts). PAE de 32 bits sin modo largo en VT-x (los PDPTE no se cargan). INS y OUTS. Los
    registros de depuración no se cambian de mundo (DR0 a DR3 y DR6 los ve el invitado siguiente; DR7 sí es de cada
    uno). En AMD, CR4 no se vigila (un invitado puede encender PKE o CET, aunque CPUID no los da). Un triple fallo mata
    la máquina en vez de reiniciarla.
- **Espacio.** La mitad baja es PML4[0], los mismos 512 GB. En vez de ASID, PCID de 12 bits; cada espacio tiene el
  suyo (como mucho 4095 procesos vivos a la vez; el 0 es del kernel), pero **PCID va apagado de serie** (decisión del
  usuario del 2026-11-15: ese camino no se ha probado en ninguna CPU): cada cambio de proceso vacía la TLB de
  usuario. Se enciende al compilar el kernel con la opción `pcid` (`make -C kernel arranque-x86 PCID=1`), si la CPU
  lo tiene.
- **FPU y SIMD.** Cada hilo tiene los suyos: x87, SSE y AVX (y AVX-512 si la CPU lo tiene y cabe en 4 KB), guardados
  con XSAVE (FXSAVE si no hay). La primera vez: registros a cero, MXCSR 0x1F80 y control del x87 0x37F (redondeo al
  más cercano, sin excepciones). Se guardan solo cuando otro hilo del mismo núcleo usa la FPU (CR0.TS, como C28).
- **TLS.** Las bases de FS y GS son de cada hilo, empiezan a 0 y el kernel las guarda y repone en cada cambio. El
  proceso las lee y las pone con `rdfsbase`, `wrfsbase`, `rdgsbase` y `wrgsbase` (FSGSBASE). En una CPU sin FSGSBASE
  esas instrucciones terminan el proceso, y no hay otra forma de ponerlas.
- **Lo que puede tocar.** `rdtsc` sí (como el contador virtual en ARM). La E/S por los puertos que le den con una llave
  de puertos (`puertos_permitir`, X6) también. `rdpmc`, `wrmsr`, `rdmsr`, `hlt`, `cli`, la E/S por cualquier otro
  puerto y cualquier otra instrucción privilegiada terminan el proceso, como leer, escribir o ejecutar la mitad
  alta. Un fallo de la FPU (#MF, #XM) también. El kernel escribe en la consola el vector, `rip`, CR2 y el código de
  error.
- **Copias.** El kernel lee y escribe la memoria del proceso con `rep movsb` entre `stac` y `clac` (SMAP), con el espacio
  del proceso puesto solo durante la copia. Con CR0.WP, no escribe donde el proceso no puede escribir. Fuera de la
  copia, la mitad baja del kernel está vacía, como en ARM.
- **Interrupciones de aparatos.** El número de una interrupción es su GSI del IOAPIC más 32 (de la 32 a la 159); llega
  al núcleo 0. Las MSI y MSI-X del PCIe (X5) se numeran como las LPI de ARM, de la 8192 a la 8255, y se tratan igual:
  se enmascaran en el kernel, una que llega enmascarada se avisa al terminar. Van al núcleo 0, cada una por su vector
  (de 0xB0 a 0xEF). El mensaje es el mismo: 4 bytes con el número, en little-endian.
- **Llaves de arranque** (X5 y X6, `kernel/src/inicio_x86.rs`). El mismo paquete que en ARM, con los ELF de x86. Los
  aparatos salen del PCIe (el ECAM de la MCFG de ACPI, todos sus buses): el xHCI (clase 0x0C0330, BAR 0), el NVMe
  (0x010802, BAR 0) y el sonido de virtio (0x1af4:0x1059, BAR 4, más su configuración), cada uno con sus registros,
  un permiso DMA propio y su MSI (MSI-X si la tiene, en su primera entrada; si no, MSI). Sin INTx: en un PC su
  número sale del AML de ACPI (`_PRT`), que el kernel no lee. Además (X6): el registro y los puertos de COM1 (tipo
  12, "puertos de E/S"; era el 7 hasta la fusión con `revision`, donde el 7 es el permiso de plazo) para la consola, que es un proceso como en ARM; y la pantalla del GOP que deja el cargador en
  la `InfoArranque`, como memoria del firmware ("pantalla") y sus medidas ("pantalla-info"), con el mismo formato que
  el `simple-framebuffer` de `virt` (U4). El formato del GOP va al revés que el de ese nodo: 1 (B, G, R en memoria)
  es a8r8g8b8.
- **Registros y memoria del firmware.** `registros_proyectar` solo proyecta lo que está fuera de la RAM, sin caché.
  La memoria del firmware (la pantalla del GOP), fuera de la RAM o en una reserva, como memoria normal: la caché la
  deciden los MTRR del firmware. La memoria DMA va con caché: en un PC el DMA ve la caché de la CPU. Las BAR pueden
  estar por encima de 4 GB (en QEMU `q35`, en 768 GB).
- **IOMMU** (X5b). Si el firmware da la DMAR (VT-d de Intel) o la IVRS (AMD-Vi), el kernel enciende la IOMMU al
  arrancar con todos los aparatos bloqueados, y cada aparato con permiso DMA tiene su dominio: el aparato solo alcanza
  la memoria DMA pedida con su permiso (`memoria_dma`), y la que el firmware reserve para él (RMRR o IVMD). La
  dirección de bus sigue siendo la física, así que el controlador no cambia. Lo que se suelta va a la cuarentena y
  sigue al alcance del aparato; con `dma_liberar` sale de su dominio antes de volver al sistema. Un DMA fuera de eso
  falla: el aparato no lee nada o no escribe, el kernel lo cuenta y escribe en la consola el aparato, la dirección y el
  motivo, y sigue. El controlador no recibe aviso (lo que haga el aparato con el error depende de él: el NVMe de QEMU
  contesta como si nada). Un aparato de virtio debe aceptar `VIRTIO_F_ACCESS_PLATFORM` (bit 33) si lo ofrece.
  Las MSI no pasan por la IOMMU (sin remapeo de interrupciones).
- **Sin IOMMU** (X5): sin DMAR ni IVRS, el permiso DMA da memoria por debajo de 4 GB, pero un aparato con el bus
  master encendido alcanza toda la memoria física: un controlador de un aparato del PCIe puede leer y escribir
  cualquier cosa por DMA. En ARM (`virt`) pasa lo mismo sin SMMU; en la Pi, el PCIe se limita al primer GB (C51).

La biblioteca `usuario/` compila para `x86_64-unknown-none` (sin SSE en el código de Rust, como el kernel; la FPU se
usa con ensamblador) con `-C relocation-model=static -C code-model=small` y su propio guion de enlace
(`usuario/enlace_x86.ld`: la GOT va con los datos), y su `_start` de ensamblador guarda la frecuencia del contador. Los programas que tocan aparatos de la Pi no se compilan para x86. Las barreras para la
memoria que se comparte con un aparato están en `dashos::barrera` (`dmb` en ARM; en x86, una barrera del compilador o
`mfence`).

## Limitaciones conocidas

- Si el montón del kernel se queda sin memoria, el kernel cae. Un proceso en un grupo con cupo no puede llevarlo
  ahí; los del grupo raíz (los que lanza el kernel) no tienen cupo.
- Una llave de registros da páginas enteras de 4 KB. La del controlador serie incluye, en la misma página, las UART 2
  a 5 de la BCM2711; son del mismo controlador. La del buzón incluye el controlador de interrupciones antiguo, el
  temporizador ARM y otros buzones de la BCM2711: falta comprobar en la Pi que no molestan con el GIC en marcha
  (auditoría de la fase 4, B8).
- `inicio` sondea la V3D al arrancar; en QEMU es un fallo de bus que se recoge. En la Pi, con el dominio de la V3D
  apagado, falta comprobar que leer no cuelga el bus (B9).
- Un temporizador puede dar un aviso más justo después de cerrar su última llave, si ya estaba saliendo (B10).
- Con la llave del canal revocable, un temporizador o una interrupción dejan de avisar también si se suelta ese
  intermediario (no solo si se revoca): no lo sujetan (segunda vuelta de la auditoría, C1).
- Un temporizador periódico que no se puede volver a programar por falta de memoria del kernel se para sin aviso.
- `dma_liberar` devuelve toda la cuarentena de un permiso de una vez; no hay forma de devolver un búfer suelto
  mientras el aparato sigue en marcha.
- El PCIe (C51): el VL805 alcanza todo el primer GB, también la memoria de la VideoCore; la llave de los registros
  del controlador PCIe no se da a ningún proceso.
- Hilos de usuario sin "soltar" (detach), sin prioridad ni afinidad por hilo, y la CPU se reparte por hilo, no por
  proceso.
- El azar de los PIE sale del contador del sistema y no es criptográfico. En la Pi 4 habrá que sembrarlo con el
  generador por hardware (RNG200).
