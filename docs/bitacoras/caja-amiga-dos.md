# Bitácora: dos.library nativa de la caja Amiga

Rama `caja-amiga-dos`, desde `cb104b1` de `revision`. Crate `caja/dos/` (`dashos-caja-dos`, biblioteca `caja_dos`):
la dos.library de la caja Amiga (S4 §4, S4b, S5) como biblioteca de Rust `no_std` con `alloc`, sin `unsafe`, probada
en el PC. Otras sesiones hacen a la vez el JIT en el proceso y una dos mínima (`caja-amiga`), exec (`caja-amiga-exec`)
y el cargador hunk (`caja-amiga-hunk`). Esto es la dos de verdad, aparte, lista para enchufar.

Sala limpia: escrito desde los autodocs de dos.library 40, el ROM Kernel Reference Manual y los includes del NDK 3.1
(`dos/dos.h`, `dos/dosextens.h`, `dos/rdargs.h`, `dos/var.h`, `dos/exall.h`, `dos_lib.fd`) como especificación de la
interfaz. Sin código de AmigaOS, AROS, UAE ni Amiberry. Sin ROM.

## Qué hay

- **El servicio de archivos, como rasgo** (`fs.rs`): `SistemaArchivos`, un método por orden del protocolo de
  `usuario/src/protocolo/archivos.rs` (buscar, mirar = buscar con a = 1, leer, escribir, listar, datos, cerrar, crear,
  borrar, renombrar, tamaño, vaciar, info, comentario, comentar, proteger), con sus estados de error (`ErrorFs`, del -1
  al -14) y `Datos` sacado de la `c` de la respuesta. `fechar` no tiene orden en el servicio: por defecto da
  `NoSoportado`. `MemFs` (`memfs.rs`) lo imita en memoria para las pruebas: asas 0-15 de raíz, sin distinguir
  mayúsculas, lo borrado sigue vivo por sus asas, permisos del dueño, volúmenes de solo lectura, disco lleno, tope de
  asas.
- **Estructuras de AmigaOS 3.1** (`estructuras.rs`): DateStamp (12), FileInfoBlock (260), FileHandle (44), FileLock
  (20), InfoData (36), DosList (44, con las tres uniones), AssignList (8), Process (228), CommandLineInterface (64),
  CSource y RDArgs (12 y 32), ExAllControl y ExAllData (`exall.rs`), y las constantes (modos, `FIBF_*`, `ST_*`, `LDF_*`,
  `DLT_*`, `GVF_*`, `DOS_*`). Los errores de `dos/dos.h` y sus textos para Fault (`errores.rs`).
- **La tabla de saltos** (`lvo.rs`): las 162 entradas de `dos_lib.fd` (Open en -30 ... SetOwner en -996) con sus
  registros, y una constante por función.
- **Rutas y nombres** (`nombres.rs`, `espacio.rs`): `Volumen:cajón/fichero`, `:` (raíz del volumen del cajón actual),
  `/` al principio o doble para subir, `/` final que no hace nada, sin distinguir mayúsculas (Latin-1 en la caja,
  UTF-8 en el servicio). El cerrojo 0 y el cajón actual 0 son la raíz del volumen de arranque.
- **Volúmenes y asignaciones** (S4b): `montar` (una raíz del servicio con nombre), AssignLock, AssignAdd
  (multiasignación, se busca en orden), AssignLate (se fija la primera vez), AssignPath (se resuelve cada vez),
  RemAssignList, `PROGDIR:` (`pr_HomeDir`) y `asignaciones_s4b` (la tabla de capas usuario / paquetes / sistema de
  S4b, con el perfil encerrado, `Home:`, `ENV:` y `T:` en `RAM:`). Todo se refleja en `DosList` en la memoria de la
  caja (con `AssignList` y `dol_AssignName`) para LockDosList, NextDosEntry, FindDosEntry, MakeDosEntry, FreeDosEntry.
- **Funciones de dos** (todas por `Dos::llamar(caja, entrada, &mut regs)`):
  - Ficheros: Open (MODE_OLDFILE, MODE_NEWFILE, MODE_READWRITE; `*`, `CONSOLE:`, `CON:`, `RAW:` y `NIL:`), Close,
    Read, Write, Seek, SetFileSize, Input, Output, SelectInput, SelectOutput, IsInteractive, SetMode, SetVBuf,
    WaitForChar.
  - Con búfer: FGetC, FPutC, UnGetC, FRead, FWrite, FGets, FPuts, Flush, VFPrintf, VPrintf, PutStr, WriteChars.
  - Cerrojos: Lock, UnLock, DupLock, Examine, ExNext, ExAll, ExAllEnd, Info, ParentDir, CurrentDir, NameFromLock,
    SameLock, SameDevice, IsFileSystem.
  - De cerrojo a fichero y al revés: OpenFromLock, DupLockFromFH, ParentOfFH, ExamineFH, NameFromFH.
  - Cambios por nombre: CreateDir, DeleteFile, Rename, SetComment, SetProtection, SetFileDate.
  - Errores: IoErr, SetIoErr, Fault, PrintFault.
  - Texto y argumentos: ReadArgs (`/A /K /S /T /N /M /F`, alias, comillas con `*N *E ** *"`), FreeArgs, FindArg,
    StrToLong, FilePart, PathPart, AddPart, SplitName, ParsePattern, ParsePatternNoCase, MatchPattern,
    MatchPatternNoCase.
  - Variables: SetVar, GetVar, DeleteVar (globales en `ENV:`, y `ENVARC:` con GVF_SAVE_VAR; locales aparte).
  - Proceso: Cli, GetArgStr, SetArgStr, GetProgramDir, SetProgramDir, Get/SetConsoleTask, Get/SetFileSysTask,
    GetProgramName, GetCurrentDirName, GetPrompt, CheckSignal.
  - Otras: DateStamp, Delay (devuelve `Accion::Dormir`), CompareDates, AllocDosObject y FreeDosObject (FIB,
    FILEHANDLE, RDARGS, EXALLCONTROL), LoadSeg y UnLoadSeg (solo la interfaz: el formato hunk es del cargador).
  - Lo demás de la tabla da el aviso `NoImplementada`, `ERROR_NOT_IMPLEMENTED` y D0 de fallo.
- **Lo que AmigaOS no hace y esta sí**: todo puntero del programa se comprueba (página 0, fuera de la memoria, los
  últimos 64 KB, cadenas sin 0 en 4 KB); un cerrojo o fichero que dos no dio (soltado, cerrado dos veces, inventado)
  da `ERROR_INVALID_LOCK` y un aviso; un cerrojo de una asignación no lo suelta el programa; nada de lo que llega al
  servicio puede salirse de sus raíces (ni `..`, ni `:`, ni `/` dentro de un nombre, ni partes vacías, ni asas que el
  servicio no dio). Cada fallo del programa queda en `avisos()` (el registro del Enforcer de S3) con su tarea y función.

## Pruebas

`cargo test` en `caja/dos/`: 53 pruebas en 8 ficheros, todas bien (asignaciones 6, cerrojos 9, estructuras 8, ficheros
11, hostil 4, patrones 2, rutas 6, texto 7). `clippy --all-targets` sin avisos. Compila para
`aarch64-unknown-none-softfloat` y `aarch64_be-unknown-none-softfloat` (nightly, `-Z build-std=core,alloc`).

El banco (`tests/comun/mod.rs`) tiene una caja falsa que rompe la prueba si dos libera algo que no reservó o con otro
tamaño, y un espía delante de `MemFs` que apunta cada nombre sucio o asa que el servicio no dio; cada prueba mira al
final que no quede nada abierto ni en dos ni en el servicio.

Casos hostiles cubiertos: rutas con `:` de más, `/` que suben por encima del volumen, `.` y `..`, nombres de 108
caracteres (107 sí), nombres de DashOS fuera de Latin-1 o de más de 107 bytes (no se ven), asas cerradas dos veces,
leer de lo abierto para escribir (como en AmigaOS, se puede), Seek fuera del fichero, un cerrojo de algo borrado por
fuera, cerrojos soltados como cajón actual, bucles de asignaciones (`A:` → `B:` → `A:`, y una tardía que se llama a sí
misma), caminos de 840 bytes (en trozos de 224), sin memoria en la caja, sin asas en el servicio, disco lleno, solo
lectura, protegido contra escritura, lectura y borrado, y patrones hechos para atascar. `tests/hostil.rs` llama a
todas las funciones con registros al azar (240.000 llamadas, la mitad con la memoria de la caja estropeada a ratos);
con `DOS_FUZZ=500 cargo test --release` y las comprobaciones de desbordes encendidas
(`CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true`) son 20 millones: ningún pánico y el servicio no vio nada sucio. Esa
tanda larga encontró una suma sin comprobar (`ctl + 4` con `ctl = 0xFFFFFFFF` en ExAllEnd) y, al repasar, otras cuatro
del mismo tipo; arregladas.

No hay prueba del oráculo (FS-UAE con el Kickstart real, regla 5 de AGENTS.md): hace falta que la caja corra binarios
68k. Lo que se dio por bueno sin oráculo está en "Decisiones provisionales".

## Cómo se enchufa en la caja

dos es un valor de Rust (`Dos<F>`, con `F` el servicio de archivos) que vive junto a exec en el proceso de la caja.

1. **El servicio de archivos**: un adaptador que implemente `SistemaArchivos` sobre el canal "archivos" (la llave 2 del
   proceso, su sesión y su ventana). Cada método es una orden; `listar` desempaqueta las entradas de la ventana (32
   bytes de cabecera, nombre y comentario, la siguiente en múltiplo de 8). dos ya garantiza nombres limpios y caminos
   de hasta 224 bytes. `info` recibe la asa (el servicio de hoy no la mira; con FS10 debería). `fechar`: falta la
   orden en el servicio.
2. **Las raíces**: la caja recibe las suyas de `sesiones` (alta y `CEDER` a los huecos 1-15) o pide volúmenes con la
   orden 21. Por cada una, `dos.montar(caja, "SYS", asa, solo_lectura)`; el primero es el de arranque (o
   `poner_arranque`). Después, `dos.asignaciones_s4b(caja, &CapasS4b { usuario, paquetes, encerrado })`.
3. **El rasgo `Caja`** (lo que dos pide fuera de su memoria), sobre exec:
   - `Memoria`: la misma forma que la de exec; un tipo sobre los primeros 4 GB del proceso sirve para los dos.
   - `reservar(largo)`: `AllocVec(largo, MEMF_PUBLIC | MEMF_CLEAR)` de la tarea en marcha (así exec la apunta a su
     dueña); `liberar`: `FreeVec`. Alineado a 4 como mínimo (los BPTR lo necesitan).
   - `tarea_actual`: `exec.tarea_actual()`. `senales(m)`: `SetSignal(0, m)` de exec (para CheckSignal y Ctrl-C).
   - `hora`, `consola_escribir`, `consola_leer`: del proceso de la caja.
   - `cargar_segmentos(datos)` y `descargar_segmentos(seg)`: el cargador hunk (rama `caja-amiga-hunk`).
4. **La biblioteca**: `exec.crear_biblioteca_nativa("dos.library", 40, rev, &numeros, tamaño_datos)` con un número
   nativo (desde 256) por entrada 1-166. Cuando el JIT llama a uno, exec devuelve `Accion::Ajena`; la caja saca
   `(base, lvo)` con `exec.ajena(n)`, la entrada con `caja_dos::lvo::entrada(lvo)` y llama
   `dos.llamar(&mut caja, entrada, &mut regs)`. Las entradas 1-4 (Open, Close, Expunge de biblioteca) las hace exec:
   dos devuelve `Ajena`.
5. **Lo que devuelve `llamar`**: `Seguir` (volver al 68k) o `Dormir { tics }` (Delay: dormir la tarea y volver).
6. **Procesos**: al crear uno (`NT_PROCESS`, 228 bytes), la caja pone `pr_CIS` y `pr_COS` con `dos.abrir_consola`,
   `pr_CurrentDir`, `pr_HomeDir` (PROGDIR:), `pr_Arguments` (lo que lee ReadArgs) y `pr_CLI` si hay CLI. dos lee y
   escribe esos campos en la memoria de la caja; si se le llama desde una tarea que no es proceso, los guarda aparte
   y no se cae.
7. **Al acabar una tarea**: `dos.acabar_tarea(caja, t)` antes de que exec suelte su memoria (cierra sus ficheros,
   suelta sus cerrojos, olvida sus variables locales).
8. **Después de cada llamada**: `dos.tomar_avisos()` al registro del Enforcer, o a Forja.

## Decisiones provisionales

- **Errores del servicio**: no existe → `ERROR_OBJECT_NOT_FOUND` (también si falta un cajón por el camino); pasar por
  un fichero o abrir un cajón → `ERROR_OBJECT_WRONG_TYPE`; protegido → `ERROR_READ/WRITE/DELETE_PROTECTED` según la
  operación; solo lectura → `ERROR_DISK_WRITE_PROTECTED`; error del disco → `ERROR_NOT_A_DOS_DISK`; sin asas →
  `ERROR_NO_FREE_STORE`. Un volumen que no está → `ERROR_DEVICE_NOT_MOUNTED`, sin pedir "inserte el volumen".
- **`/` se resuelve en dos, sin preguntar al servicio** (que no tiene "cajón padre"): `a/nada//b` vale aunque `nada` no
  exista; en AmigaOS fallaría.
- **Nombres**: hasta 107 caracteres Latin-1 (lo que cabe en `fib_FileName`); más, o `.`/`..`, es
  `ERROR_INVALID_COMPONENT_NAME`. Lo que DashOS guarda con un nombre que la caja no puede escribir (fuera de Latin-1 o
  largo) no sale en ExNext ni en ExAll. Los comentarios se cortan a 79.
- **Cerrojos y objetos en uso**, dentro de la caja: MODE_NEWFILE y los cerrojos exclusivos no dejan a nadie más; nada
  se borra con un cerrojo o fichero encima (también los de las asignaciones). Lo que hagan otros programas de DashOS
  no se ve aquí: eso lo lleva el servicio.
- **Capas al escribir** (S4b): Open NEWFILE/READWRITE y CreateDir prueban las capas de una multiasignación en orden y
  saltan las de solo lectura, protegidas o sin el cajón; lo demás (DeleteFile, Rename, SetComment...) actúa en la
  primera capa donde está el objeto.
- `Rename` de un cajón dentro de sí mismo → `ERROR_OBJECT_IN_USE`. Cambiar solo mayúsculas sí se puede.
- `Close` de un fichero ya cerrado → DOSFALSE con `ERROR_INVALID_LOCK` y aviso; `Close(0)` no hace nada.
- `fl_Key` es un número del objeto (el mismo para dos cerrojos del mismo); `fl_Task` y `fh_Type` apuntan a un
  `MsgPort` vacío por volumen: no recibe paquetes (DoPkt a él no va). `id_DiskType` es `DOS\3` (FFS internacional).
- Las funciones con búfer van directas, sin búfer propio, salvo el carácter de UnGetC; Flush lo devuelve al fichero.
- Seek más allá del final → `ERROR_SEEK_ERROR` y la posición no cambia.
- ReadArgs lee de `pr_Arguments` (o del `CSource`), no del búfer de `Input()`; `?` no pide la línea otra vez.
- VPrintf/VFPrintf siguen a RawDoFmt (sin `l`, WORD); `%x` en minúsculas y `%X` en mayúsculas.
- El patrón "analizado" de ParsePattern es propio (marca y el patrón tal cual). Patrones de más de 128 nodos no se
  aceptan; textos de más de 256 caracteres no casan. `*` es un carácter normal.
- Variables locales: de cada tarea, guardadas en dos; no aparecen en `pr_LocalVars` (FindVar da 0).
- AddPart pone `/` entre las dos partes aunque el nombre empiece por `/` (lo que queda sube un cajón al resolverse).
- `PROGDIR` no se puede asignar. El primer volumen montado es el de arranque.

## Lo que falta

- MatchFirst, MatchNext, MatchEnd (AnchorPath).
- Lanzar programas: SystemTagList, Execute, RunCommand, CreateNewProc, CreateProc, Exit (con exec, el cargador y un
  shell). Hoy dan aviso y `ERROR_NOT_IMPLEMENTED`.
- Paquetes: DoPkt, SendPkt, WaitPkt, ReplyPkt, AbortPkt, DeviceProc, GetDeviceProc, FreeDeviceProc. Haría falta un
  gestor de paquetes por volumen que traduzca a `SistemaArchivos`.
- StartNotify/EndNotify (con los avisos del servicio), LockRecord(s)/UnLockRecord(s), MakeLink/ReadLink, SetOwner,
  Format, Relabel, Inhibit, AddBuffers, DateToStr/StrToDate, InternalLoadSeg/NewLoadSeg, la lista de residentes
  (AddSegment, FindSegment, RemSegment), SetProgramName/SetCurrentDirName/SetPrompt, FindVar, AddDosEntry/RemDosEntry,
  ReadItem, VFWritef, CliInitNewcli/CliInitRun, ErrorReport.
- La base de dos (`DosLibrary`, `dl_Root`, `RootNode`, `DosInfo`): un programa que recorra la lista de dos a mano
  desde `DOSBase` ve ceros.
- E/S sin bloquear la caja: hoy cada orden al servicio espera con el testigo de la caja; un disco lento para a las
  demás tareas 68k. WaitForChar siempre dice que no hay nada.
- La consola: `CON:` y `RAW:` van a la consola de la caja en vez de abrir una ventana; SetMode no cambia nada.
- La prueba del oráculo de cada función, cuando la caja ejecute binarios 68k.
