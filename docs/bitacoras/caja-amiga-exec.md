# Bitácora: exec.library nativa de la caja Amiga

Rama `caja-amiga-exec`, desde `cb104b1` de `revision`. Crate `caja/exec/` (`dashos-caja-exec`, biblioteca `caja_exec`):
el núcleo de exec para la caja Amiga (S4, S2, S3), como biblioteca de Rust `no_std` con `alloc`, sin `unsafe`, probada
en el PC. Otra sesión (rama `caja-amiga`) mete el JIT en un proceso de DashOS; esto es lo que se enchufa allí.

Sala limpia: escrito desde el ROM Kernel Reference Manual (Libraries), los autodocs de exec y los includes del NDK 3.1
(`exec/*.h`, `exec_lib.fd`) como especificación de la interfaz. Sin código de AmigaOS, AROS, UAE ni Amiberry. Sin ROM.

## Qué hay

- **Memoria de la caja** (`memoria.rs`): el rasgo `Memoria` (dar trozos `[dir, dir+largo)` de los 4 GB del 68k) con
  lecturas y escrituras big-endian de 8, 16 y 32 bits. `MemoriaVec` es un `Vec<u8>` para las pruebas. Fuera de la
  memoria da `Fallo::FueraDeMemoria`. Encima, exec aplica el Enforcer: nada en la página 0 (4 KB) salvo leer la base en
  la dirección 4, y nada en los últimos 64 KB.
- **Estructuras de AmigaOS 3.1** (`estructuras.rs`): Node, MinNode, List, MinList, Task, MsgPort, Message, Library,
  MemChunk, MemHeader, MemEntry/MemList, SignalSemaphore, Interrupt/IntVector/SoftIntList y ExecBase (V40, 632 bytes),
  con los nombres de campo de los includes. Las pruebas fijan tamaños y desplazamientos.
- **LVO** (`lvo.rs`): las 137 entradas de `exec_lib.fd` (-6 a -822) con sus registros.
- **Registros** (`registros.rs`): D0-D7/A0-A7 y la conversión con la copia de x0..x30 que hace el JIT
  (`patches/emu68/0002`: D0-D7 en x19-x26, A0-A4 en x13-x17, A5 x27, A6 x28, A7 x29).
- **Funciones de exec** (el número es el de la entrada; todas por `Exec::llamar(n, &mut regs)` y también como métodos):
  - Listas: Insert, AddHead, AddTail, Remove, RemHead, RemTail, Enqueue, FindName (y NewList).
  - Memoria: Allocate, Deallocate, AllocMem (MEMF_CLEAR, MEMF_REVERSE, atributos), AllocAbs, FreeMem, AvailMem
    (MEMF_LARGEST, MEMF_TOTAL), AllocEntry, FreeEntry, AllocVec, FreeVec, TypeOfMem, CopyMem, CopyMemQuick.
  - Tareas: AddTask, RemTask, FindTask, SetTaskPri; señales: SetSignal, Wait, Signal, AllocSignal, FreeSignal.
  - Forbid, Permit, Disable, Enable.
  - Puertos: CreateMsgPort, DeleteMsgPort, AddPort, RemPort, FindPort, PutMsg, GetMsg, ReplyMsg, WaitPort.
  - Bibliotecas: OpenLibrary, OldOpenLibrary, CloseLibrary, AddLibrary, RemLibrary, MakeLibrary, MakeFunctions,
    InitStruct, SetFunction, SumLibrary; Open/Close/Expunge genéricas para las bibliotecas nativas.
  - CacheClearU y CacheClearE (apuntan qué debe olvidar el JIT).
  - Lo demás de la tabla da el aviso `NoImplementada` y D0 = 0.
- **Lo que exec no hace y este sí** (S2, S3): tabla nativa de bloques con su tarea dueña; `FreeMem` de algo no
  reservado, liberar dos veces o con otro tamaño se niega con un aviso; las listas se comprueban antes de escribir
  (un `Remove` doble o una lista en círculo dan `ListaRota`); `PutMsg` a un puerto borrado, de un mensaje en memoria
  libre o que ya está en una cola, se niega. `matar_tarea` (S2) suelta bibliotecas, puertos y memoria de una tarea.
- **El planificador** (`tareas.rs`): `elegir_siguiente()` decide quién tiene el testigo: la de más prioridad de
  `TaskReady`; entre iguales, por turnos al acabar el turno (`fin_de_turno()`). Respeta `Forbid` y `Disable` (los
  contadores de ExecBase, que los programas suben a mano con las macros de `exec/ables.i`); si la tarea espera dentro
  de un `Forbid`, este se rompe mientras espera y vuelve con ella.

## Pruebas

`cargo test` en `caja/exec/`: 95 pruebas en 7 ficheros (estructuras 9, listas 14, memoria 18, tareas 24, puertos 14,
bibliotecas 13, hostil 3). Cada grupo tiene sus casos hostiles: puntero 0, nodo fuera de la memoria o en la página 0,
FreeMem de algo no reservado, liberar dos veces, señal no reservada, mensaje a un puerto borrado, cerrar de más,
SetFunction con un LVO malo, tablas de InitStruct y vectores rotas, listas y MemHeader estropeados.
`tests/hostil.rs` llama a todas las funciones con registros al azar (unas 340.000 llamadas, la mitad con la memoria del
sistema estropeada a ratos) en modo de depuración, con comprobación de desbordes: ningún pánico.
`clippy --all-targets` sin avisos. Compila para `aarch64-unknown-none-softfloat` y `aarch64_be-unknown-none-softfloat`
(nightly, `-Z build-std=core,alloc`).

No hay aún prueba del oráculo (FS-UAE con el Kickstart real, regla 5 de AGENTS.md): hace falta que la caja corra
binarios 68k. Lo que se dio por bueno sin oráculo está en "Decisiones provisionales".

## Cómo se enchufa en la caja

Exec es un valor de Rust (`Exec<M>`); quien tiene el testigo de la caja tiene `&mut Exec`. Lo que espera del proceso:

1. **Memoria**: un tipo que implemente `Memoria` sobre los primeros 4 GB del proceso (el `unsafe` va en la caja, no
   aquí). `Exec::arrancar(mem, Config::para(tamaño))` monta ExecBase en `0x1000-0x4000`, las regiones repartibles
   (chip hasta 2 MB, fast encima) y escribe la base en la dirección 4. Después la caja protege la página 0.
2. **La línea A**: en `dashos_native_call(regs, n)`:
   ```
   let mut r = Registros::desde_jit(regs);
   let mut acc = if n <= lvo::ULTIMO_DE_EXEC { exec.llamar(n, &mut r) } else { Accion::Ajena };
   loop { match acc {
       Accion::Llamar { destino, regs: rr, tras } => {   // código 68k (llamar_68k) u otra biblioteca nativa
           let hecho = ejecutar(destino, rr);              // sin tener prestado exec
           acc = exec.continuar(tras, &hecho, &mut r);
       }
       Accion::Ajena => { /* a la biblioteca nativa que tenga ese número: exec.ajena(n) */ }
       _ => break,
   }}
   r.hacia_jit(regs);
   ```
3. **El testigo**: `Seguir`, volver al 68k. `Ceder`, la llamada acabó pero hay una tarea de más prioridad: pasar el
   testigo a `exec.elegir_siguiente()` y, al recuperarlo, volver al 68k. `Bloquear` (Wait, WaitPort): pasar el testigo
   y, al recuperarlo, **repetir la misma llamada con los registros de antes** (exec sabe que es la repetición).
   `Terminar`: la tarea se ha quitado; su hilo acaba tras pasar el testigo. `elegir_siguiente()` da `None` si no hay
   ninguna lista: la caja duerme hasta que algo de fuera llame a `exec.signal(...)`.
4. **Tareas nuevas**: tras un `AddTask` (del 68k o `exec.crear_tarea`), `exec.arranque(t)` da `(pc, pila)` para crear
   su hilo (A7 = pila, PC = pc); el hilo espera hasta que `elegir_siguiente()` lo elija. Al volver de su función, la
   tarea salta al código de fin de exec (`$A0FF 200 RTS`, `lvo::FIN_DE_TAREA`), que la quita.
5. **El reloj**: al acabar cada turno, `exec.fin_de_turno()`; si da `true`, quitarle el testigo a la tarea (S4: el
   hilo se detiene donde esté) y llamar a `elegir_siguiente()`. Las interrupciones simuladas (barrido vertical) solo
   si `exec.interrupciones_activas()`.
6. **Después de cada llamada**: `tomar_codigo_cambiado()` (el JIT olvida esos tramos; `(0, 0)` es todo),
   `tomar_quitadas()` (acabar esos hilos) y `tomar_avisos()` (al registro del Enforcer, o a Forja).
7. **Bibliotecas nativas** (dos, graphics... cuando existan): `exec.crear_biblioteca_nativa(nombre, versión,
   revisión, &números, tamaño_datos)`, con números mayores que 255; la caja recibe esos números como `Ajena` y sabe de
   qué biblioteca y LVO son con `exec.ajena(n)`.
8. **Matar una tarea** (S2): `exec.senal_ctrl_c(t)`, esperar, y si no sale, detener su hilo y `exec.matar_tarea(t)`.

## Decisiones provisionales

- **Formato de la entrada nativa**: `$A0FF`, número de 16 bits y `RTS`, como el parche `0002-llamada-nativa-linea-a`
  (el encargo hablaba de un número de 32 bits; con 6 bytes por entrada no cabe el `RTS`). Si el JIT cambia, solo hay
  que tocar `escribir_entrada_nativa` y `entrada`. Números: 1-137 son las entradas de exec, 200 el fin de tarea, hasta
  255 reservados para exec; desde 256, otras bibliotecas.
- `SetFunction` sobre una entrada nativa devuelve su **trampolín** (`$A0FF n RTS` fijo), para que el parche pueda
  llamar a la original.
- Las funciones que bloquean se **repiten** al volver el testigo, en vez de guardar a medias su estado.
- `FreeMem(0)`, `FreeVec(0)`, `CloseLibrary(0)` y `DeleteMsgPort(0)` no hacen nada. `FreeMem` con otro tamaño (tras
  redondear a 8) se niega: AmigaOS dejaría liberar un trozo de un bloque, y aquí no.
- `AllocVec` guarda el tamaño total en el LONG de antes; su memoria queda alineada a 4, no a 8.
- `AllocSignal` deja la señal sin recibir. `AddTask` pone `tc_SigAlloc` de `TaskSigAlloc` (0xFFFF) si viene a 0,
  respeta `NT_PROCESS` y mete en la pila la dirección de vuelta.
- `WaitPort` con un mensaje ya en la cola no gasta la señal; si esperó, sí.
- `PutMsg` a un puerto sin `NT_MSGPORT` avisa pero entrega (hay programas que montan puertos a mano).
- `InitStruct`: cada orden empieza en dirección par y los datos de WORD y LONG van alineados a 2 (lo que piden las
  macros de `exec/initializers.h`). A comprobar con el oráculo.
- Las bibliotecas nativas no se van de memoria (`Expunge` solo pone `LIBF_DELEXP`).
- `RemTask` suelta solo lo de `tc_MemEntry`, como exec; `matar_tarea` suelta todo lo de la tarea.
- exec dice ser la 40.1. La memoria "chip" existe porque los programas la piden; no hay chipset detrás.

## Lo que falta

- Semáforos (InitSemaphore, Obtain/Release/Attempt, compartidos, listas), Cause y las interrupciones por software
  (`PA_SOFTINT`), servidores de interrupción, excepciones de tarea (SetExcept, `tc_SigExcept`), AllocTrap/FreeTrap.
- Dispositivos: OpenDevice, CloseDevice, DoIO, SendIO, CheckIO, WaitIO, AbortIO, CreateIORequest, DeleteIORequest.
- Pools (CreatePool, AllocPooled...), RawDoFmt, Supervisor/SetSR/GetCC, Alert, StackSwap, manejadores de memoria,
  AddMemList, CacheControl, ColdReboot, los Child*.
- Cargar bibliotecas de `LIBS:` (ramlib, con dos) cuando `OpenLibrary` no la encuentra.
- Mensajes en vuelo de una tarea muerta: su memoria se suelta igual (S2 pide mantenerla hasta que vuelvan).
- La prueba del oráculo de cada función, cuando la caja ejecute binarios 68k.
