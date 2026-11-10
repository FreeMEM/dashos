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
| R1 | Cliente hostil en las pruebas del arranque | B6 | pendiente |
| R2 | Servicios: mensajes vacíos, fugas de llaves, giros, topes de Wayland, tecla perdida | E1 a E5 | pendiente |
| R3 | Kernel: dormidos sin tope, temporizador puntual, interrupción con la cola llena, aviso perdido | K9 a K12, K15 | pendiente |
| R4 | Llaves de aparato con derechos mínimos, USB hostil, foco del teclado, TIC del sonido | S4, S7, S8 | pendiente |
| R5 | Clase de plazo con llave y admisión que cuenta a los dormidos | S3, K8 | pendiente |
| R6 | Archivos con ficha: dueño, grupo y modo, raíces, ceder, administrar | U1, A13 | pendiente |
| R7 | Banco fiable | B1 a B4 | pendiente |
| R8 | Archivos, lo rápido: cerrar sin guardar, menos viajes | A1 a A6 | pendiente |
| R9 | Aviso de muerte y plazo al leer | K7 | pendiente |
| R10 | Azar de la placa y arranque endurecido | S5, S6 | pendiente |
| R11 | Kernel sin pánicos por memoria y llamadas largas troceadas | S1, S2 | pendiente |
| R12 | Supervisión de servicios y biblioteca común | E6 a E9 | pendiente |
| R13 | Discos ajenos en un proceso lector aparte | S6b regla 4 | pendiente |
| R14 | `sesiones`: cuentas, fichas, lanzar desde disco, procesos | U2 | pendiente |
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
