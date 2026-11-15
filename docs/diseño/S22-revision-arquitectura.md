# S22. Revisión de arquitectura: DashOS, RelevoJIT y Dash en ARM y x86-64

Del 2026-11-15. La pidió el usuario. Prioridades, por orden: compatibilidad, estabilidad, velocidad. Papel: analista.
No se cambió código ni se compiló ni se arrancó nada: todo sale de leer.

Estado leído:

- DashOS, rama `x86` (343b57c) en `.worktrees/x86`. Sus cambios sin commit (X7a) no se juzgan.
- DashOS, rama `revision` (2026-11-15 01:01), solo con `git show`. Ahí vive la caja Amiga de verdad.
- RelevoJIT, `main` (98e774e). Emu68, af63580 con los parches de `patches/emu68/`.
- Dash (`~/Projects/FreeMEMLang`, `main`, 71972a1).

Marcas: **[C]** comprobado en el código. **[D]** leído en un documento o bitácora. **[S]** supuesto mío.

Aviso de nombres: en `revision` ya existen `S21-cada-trabajo-a-su-chip.md` y `S22-suspender-hibernar.md` [C]. Este
fichero y `S21-x86.md` chocan de número con ellos. Hay que renumerar al fusionar.

## Resumen: los hallazgos, por gravedad

1. **La rama `x86` sale de un kernel viejo.** Parte de `main` (3170b53, del 9 de noviembre). `revision` lleva 632
   commits más; `x86`, 16 [C]. La caja Amiga, los hilos de usuario, las máquinas virtuales y el permiso de ejecutar
   están solo en `revision`. Hay 29 ficheros cambiados en las dos; `planificador.rs` tiene +1000 líneas allí y +195
   aquí [C]. Importa porque X7 necesita la caja, y cada día la fusión cuesta más. Propuesta: fusionar `revision` en
   `x86` antes de seguir con X7.
2. **RelevoJIT no deja volver a entrar, y la caja lo necesita.** `relevo.h:89` dice que una función del anfitrión
   no puede llamar a `relevo_run()` otra vez [C]. La caja ejecuta 68k desde funciones nativas, hasta 16 niveles
   (`caja_ejecutar_68k`, `caja/caja.c:546` en `revision`) [C]: ganchos de MUI e Intuition, `tc_TrapCode`. Sin eso
   no abre ni una ventana de MUI. Propuesta: una entrada anidada oficial en RelevoJIT, con pruebas.
3. **`caja.c` está cosida a las tripas de Emu68.** Usa sus listas internas, registros ARM fijos y ensamblador a
   mano [C]. No hay interfaz de motor. Propuesta: definirla ahora (sección 3) y meter Emu68 detrás de ella.
4. **Las bibliotecas de la caja ya son Rust y no dependen del orden de bytes.** `PLAN.md` dice que se escriben en
   Dash. En `revision` son crates de Rust que leen la memoria del 68k con `from_be_bytes` [C]. Sirven en x86-64
   casi tal cual. El punto "crítico" de Dash queda mucho más pequeño. Propuesta: aceptarlo en el plan (sección 4).
5. **Un fallo de página mata el proceso entero, en las dos arquitecturas** [C]. Con Emu68, un programa 68k que
   escribe donde no debe tumba la caja con todas sus tareas (`caja.c:1640`) [C]. S18 y el paso V6 de RelevoJIT
   piden entregar el fallo al proceso, y no está hecho. RelevoJIT sí da un error de bus del 68k sin ayuda del
   kernel, porque comprueba cada acceso. Es su mayor ventaja de estabilidad.
6. **RelevoJIT no acepta la memoria en la dirección 0** (`cpu.c:21`) [C]. La caja, las bibliotecas y Dash dan por
   hecho que la dirección 68k es la del proceso. Propuesta: que RelevoJIT admita base 0.
7. **RelevoJIT no ha corrido un programa de verdad** [D, su `REVISION.md`]. Emu68 ya mueve MUI 5 y Tachyon en la
   caja [D]. No se puede cambiar de motor en ARM todavía.
8. **Vaciado de TLB en x86: si un núcleo no contesta en 100 ms, se avisa y se sigue** (`nucleos.rs:147`) [C]. La
   memoria se suelta con una traducción quizá viva en otro núcleo. En una máquina virtual cargada, 100 ms sin
   contestar pasa. Propuesta: no soltar la memoria hasta que contesten todos.
9. **PCID encendido y sin probar** [D, `X86_LOOP.md:243`]. Un fallo ahí da memoria de otro proceso. Propuesta:
   apagado por defecto hasta probarlo en una CPU de verdad.
10. **Carrera pequeña en el aviso de interrupción de RelevoJIT** (`jit.c:613`) [C leído, sin provocar]. Una
    interrupción puede esperar hasta que se acabe el presupuesto. Propuesta: una escritura con barrera.
11. **Dash para x86-64 no existe, pero el camino por C es corto.** Falta un objetivo de clang y unas 60 líneas de
    ensamblador en `runtime/dashos/plat.c` [C]. El camino directo pide un generador nuevo.
12. **Dos pilas de parches y dos toolchains big-endian que x86 no puede usar.** Emu68 lleva 14 parches, unas
    7200 líneas [C]. Con un solo motor que gire los bytes, la caja sería little-endian en todas partes.

## 1. DashOS: arquitectura general

### Capas y fronteras tras X1 a X6

- Lo de x86 está en `kernel/src/base/x86_64/`. Lo de ARM sigue suelto en `kernel/src/base/`, no en
  `base/aarch64/` como pedía S21 [C]. Fue a propósito, para tocar poco (`X86_LOOP.md`) [D].
- x86 reutiliza cuatro ficheros de ARM con `#[path]`: `cerrojo.rs`, `memoria.rs`, `monton.rs` y `planificador.rs`
  (`base/x86_64/mod.rs`) [C]. Es frágil: `planificador.rs` tiene 33 `cfg` de arquitectura [C]. Mezcla el reparto
  de hilos, que es común, con el contexto, la FPU, el TLS y la bajada a usuario, que no lo son.
- `cfg` de arquitectura fuera de `base`: `main.rs` 21, `serie.rs` 15, `llamadas.rs` 5, `interrupciones.rs` 4,
  `reloj.rs` 4 [C]. En `usuario/`: `fpu_comun.rs` 13, `lib.rs` 11, `barrera.rs` 9 [C].
- Interfaces de `base` que no son comunes de verdad:
  - Los nombres son de ARM: `Espacio::ttbr0()`, `asid()`, `Marco::poner_x0()` [C]. Funciona, pero engaña.
  - `Permiso::DatosSinCache` no significa nada en x86 (va como memoria normal) [C].
  - Las interrupciones se numeran distinto: GSI más 32 y MSI desde 8192 en x86; SPI y LPI en ARM [D].
  - `puertos_permitir` (llamada 29) solo existe en x86; en ARM da `E_TIPO` [D].
  - El orden de bytes por proceso (`orden_el0`, `Proceso.big_endian`) solo existe en ARM [C].

Propuesta, tras la fusión: mover lo de ARM a `base/aarch64/` y partir `planificador.rs` en dos. Una parte común
(colas, clases, plazos) y un `arq` por arquitectura con `Contexto`, `EstadoFp`, TLS y entrada a usuario. Así los
33 `cfg` quedan en uno. Hacerlo antes de la fusión la complica; después, es mecánico.

### La fusión pendiente (hallazgo 1)

- Ficheros que más chocan [C]: `planificador.rs` (+195 aquí, +1000 allí), `llamadas.rs` (+38, +765),
  `memoria.rs` (+115, +261), `usuario/src/lib.rs` (+115, +356), `pruebas.rs`, `main.rs`, `kernel.md`.
- `revision` trae lo que x86 no tiene: hilos de usuario (llamadas 40 a 44), `esperar` y `despertar`, máquinas
  virtuales (45 a 48, 55), vistas de memoria (50), página cero (49), aviso de memoria (53) [C]. Todo eso necesita
  su mitad de x86. El hipervisor (`hiper.rs`, EL2) no tiene equivalente escrito: en x86 sería VT-x o AMD-V.
- El permiso de ejecutar ya está en `revision` para ARM (`llamadas.rs:450`, con `SCTLR_EL1.UCI`) [C]. X7a lo
  añade aparte en x86. No lo juzgo; solo aviso de que chocará.

### Riesgos de estabilidad

- **TLB (hallazgo 8).** `quitar_proyeccion` borra las entradas, marca el PCID como sucio, llama a
  `vaciar_tlb_otros` y suelta el objeto [C]. Si un núcleo no contesta, sigue. El vaciado en sí es correcto:
  apagar y encender `CR4.PGE` vacía todos los PCID [C, y manual de Intel]. Además vacía la TLB entera de todos
  los núcleos en cada `memoria_quitar`. Es lento, pero seguro.
- **PCID (hallazgo 9).** La lógica de "sucios" por núcleo parece correcta al leerla (`espacio.rs`, `activar`) [C].
  Pero ni QEMU TCG ni el KVM de la máquina lo dan [D]. Código de memoria sin ejecutar nunca no debe ir encendido.
- **Llamadas con las interrupciones cerradas.** Toda llamada corre así. Solo `memoria_crear` tiene pausas [D].
  `memoria_dma` pone a cero hasta 4 MB con el cerrojo del repartidor; soltar un proceso grande va de una vez [D].
  Con un PC de 16 GB y procesos grandes, eso son pausas de milisegundos para el sonido.
- **Cerrojos.** En x86, quien espera un cerrojo atiende los vaciados de TLB (`cerrojo.rs:43`) [C]. Bien pensado.
  Pero un núcleo con las interrupciones cerradas que no espera cerrojo no contesta: de ahí el tope de 100 ms.
- **Sin IOMMU.** Un aparato con bus master llega a toda la memoria [D]. En QEMU no importa. En un PC real, un
  controlador de usuario con fallo corrompe el kernel. Hay que tenerlo antes de la zona GPL (regla 7).
- **Sin INTx** (falta leer el AML) y **IOAPIC a procesos sin probar** [D]. Un PC real con un aparato sin MSI no
  funcionará.
- **FSGSBASE.** Sin él no hay TLS y el proceso muere [D]. Todas las CPU desde 2013 lo tienen. Vale.

### Decisiones de ARM: cuáles valen en x86-64

| Decisión | En x86-64 |
|---|---|
| Orden de bytes por proceso (E0E) | No existe. Todo es little-endian. Sobra `big_endian` en `Proceso` |
| U7: memoria 68k big-endian, código little-endian | Es el único camino. RelevoJIT nace así |
| La caja en los primeros 4 GB | Vale igual: la mitad baja son los mismos 512 GB [C]. Pide la llamada 49 |
| Punteros de 32 bits en estructuras Amiga | Vale. Es del formato de los datos, no de la CPU |
| Código de la caja por encima de 128 GB (S28) | Vale. RelevoJIT llama a C con direcciones de 64 bits [C] |
| Registros del 68k fijos en registros de la CPU | No vale: x86 tiene 16. RelevoJIT los reparte por bloque [C] |
| W^X con dos proyecciones del mismo objeto | Vale, y es más simple: x86 no necesita limpiar cachés |
| MTE para el Enforcer | No existe. El Enforcer va por comprobación (RelevoJIT) o por la MMU |

## 2. RelevoJIT

Su `docs/REVISION.md` ya cubre el motor por dentro: falta de programas reales, transcendentales, x87, quitar la
comprobación por acceso. Estoy de acuerdo y no lo repito. Aquí va lo que esa revisión no mira: cómo encaja.

### Lo que está bien [C]

- Dos motores, una interfaz, sin estado global. El intérprete es C11 portable.
- Un bloque nunca falla: comprueba antes cada acceso y, si algo no es RAM normal, sale con el estado intacto
  (`translate.c:783`, `jit.h`). Las excepciones las hace siempre el intérprete. Por eso salen iguales.
- El código cambiado se caza por páginas marcadas, no por suma al entrar (`jit.c`, `mark_block`).
- La línea A corta el bloque y la atiende el intérprete (`decode.c:856`, `ops_control.c:17`). Así la función
  nativa nunca corre dentro de un bloque. Esto hace fácil la entrada anidada.
- La ABI está cuidada: guarda RBX, RBP y R12 a R15, alinea la pila a 16, y guarda RSI, RDI y R8 a R11 alrededor
  de cada llamada a C (`translate.c:1685` y `2372`).

### Lo que falta o falla para ser el motor de la caja

1. **Entrada anidada** (hallazgo 2). `relevo_run` pisa `budget`, `exiting` y `fault_jmp` (`cpu.c:204`) [C]. Hace
   falta `relevo_call(cpu, pc, tope)`: guarda eso, corre hasta la vuelta y lo repone. Solo desde `line_a` y
   `line_f`. Desde `read` y `write` no, porque ahí sí hay un bloque en la pila (`jit_host_step`).
2. **Base 0** (hallazgo 6). `config_valid` rechaza `memory == NULL` con tamaño [C]. Con base 0, además, el JIT
   puede soltar R15 y ganar un registro. Si no, hay que sumar una base en la caja, en Rust y en Dash.
3. **Memoria de código en DashOS.** `codemem.c` solo tiene Windows y Unix; en otro sistema `codemem_create`
   devuelve falso y `relevo_create` da NULL [C]. Pide 32 MB por CPU (`jit_internal.h:17`); un objeto de DashOS
   mide 16 MB como mucho [D]. La caja de ARM usa 4 MB [C]. Propuesta: que el anfitrión dé las dos vistas (RW y
   RX) y el tamaño en `relevo_config`.
4. **Escrituras nativas en código.** El JIT ve las escrituras del 68k y las de `relevo_write`. Las bibliotecas
   de Rust escriben directo en la memoria (`Memoria::trozo_mut`) [C]. `CopyMem`, `LoadSeg` o `Read` sobre una
   zona con código traducido dejan traducciones viejas. Propuesta: una función barata "¿esta página tiene
   código?" y que `trozo_mut` avise. No fiarlo a que cada función se acuerde.
5. **Vaciar todo.** `jit_flush` no es pública y `relevo_invalidate` lleva un tamaño de 32 bits: no cubre 4 GB
   [C]. La caja necesita "olvidar todo" (`caja_jit_olvidar`) [C].
6. **Carrera del aviso** (hallazgo 10). El bucle pone `attention = 0` y luego lee el nivel de interrupción, las
   dos cosas sin barrera (`jit.c:613`). `relevo_set_ipl` escribe el nivel y luego `attention = 1` (`cpu.c:236`).
   Con el búfer de escrituras de la CPU, el 0 puede llegar después del 1. Bloques encadenados no vuelven al
   bucle, así que la interrupción espera al fin del presupuesto. Arreglo: `atomic_exchange` al limpiar. La caja
   usa esto para el reloj de turnos, Ctrl-C y el vigilante que corta programas colgados.
7. **Transcendentales con `long double` del anfitrión** (`fpx_trans.c:5`) [C]. En DashOS no hay libc: hay que
   traerlas. Y dan distinto en ARM (128 bits por software) y en x86 (x87). Ya está en V4.
8. **Un solo hilo por CPU.** El encadenado parchea saltos en caliente [C]. Es seguro solo si nadie más ejecuta
   bloques de esa CPU. Con el testigo de la caja se cumple. Hay que escribirlo en `relevo.h`.

### Sobre x86-64 de DashOS

- **W^X.** Dos proyecciones del mismo objeto, una para escribir y otra para ejecutar. `revision` ya lo hace en
  ARM y rechaza escribir y ejecutar a la vez [C]. RelevoJIT ya trabaja con dos vistas (`codemem.dual`) [C].
- **4 GB y fallos de página.** Hoy no hace falta reservar nada: cada acceso mira un mapa (unas 9 instrucciones
  de más, `translate.c:796`) [C]. Quitarlo (V6) pide que el kernel entregue el fallo al hilo. No existe en
  ninguna rama [C]. Es trabajo de kernel en las dos arquitecturas.
- **FPU.** Hoy no usa x87 ni SSE en el JIT [D]. Si llega V5 (x87): el kernel ya guarda x87, SSE y AVX por hilo
  con XSAVE, perezoso con `CR0.TS` (`cpu.rs:219`, `planificador.rs`) [C]. Ojo con dos cosas. Un `#MF` o `#XM`
  mata el proceso [D]: las excepciones del x87 tienen que seguir enmascaradas siempre. Y la palabra de control
  hay que reponerla al salir a C, porque `fpx_trans` usa `long double` con el control normal.
- **`MOVBE` y CPUID.** `CPUID` se puede ejecutar en el anillo 3. Se mira al crear la CPU, con `BSWAP` de reserva
  (`translate.c:2515`) [C]. Bien. Con un compilador que no sea gcc o clang, no detecta nada [C].
- **CET.** S21 prevé pila de sombra e IBT [D]. Con IBT, los saltos indirectos del JIT (caché de `RTS`) piden
  `ENDBR64` en la entrada de cada bloque. Hoy no lo pone [C]. Apuntarlo para cuando llegue S16 a x86.
- **Hilos y varias CPU.** `relevo_set_ipl` y `relevo_request_stop` son atómicas [C]. Lo demás, un hilo. La
  rama `x86` no tiene hilos de usuario (llegan con la fusión): sin ellos no hay vigilante ni reloj de turnos.

## 3. La caja Amiga con dos motores

### Cómo es hoy (rama `revision`) [C]

- Un proceso big-endian (`aarch64_be`), PIE por encima de 128 GB. El JIT de Emu68 dentro, con memoria de código
  de 4 MB en dos vistas.
- Bibliotecas nativas en Rust: `caja/exec`, `dos`, `intuition` (con graphics, GadTools, BOOPSI), `utility`,
  `timer`, unidas en `caja/pegamento` (`aarch64_be-unknown-none-softfloat`, con `build-std`).
- **Una sola CPU 68k** (`static struct M68KState m68k`, `caja.c:347`). Las tareas no son hilos: exec cambia los
  registros dentro de una llamada nativa. S4 decía "cada tarea, un hilo"; la realidad es más simple y encaja con
  un solo `relevo_cpu`.
- Lo cosido a Emu68: `fijar` y `desfijar` sacan unidades de `LRU` e `ICache` (`caja.c:389`); `main_loop_anidado`
  guarda `q8` a `q31` a mano; `sr_vivo` lee el SR de un registro vectorial; `Registros::desde_jit` conoce
  `x19`, `x13`, `x27` (`caja/exec/src/registros.rs`); el Makefile lleva 40 opciones `-ffixed-*`.

### La interfaz que hace falta

Una capa "motor" en la caja, con estas operaciones. Entre paréntesis, quién la cumple hoy.

| Operación | Emu68 | RelevoJIT |
|---|---|---|
| Registros del 68k en una estructura neutra (D, A, PC, SR, USP, ISP, MSP, VBR, FPU) | Copia de x0 a x30 y registros vivos | `relevo_get_regs` [C] |
| Llamada nativa: `(registros, número)` desde `$A0FF número` | Parche 0002 | `line_a`; el anfitrión lee el número y avanza el PC |
| Ejecutar 68k desde una función nativa, hasta su vuelta | `MainLoop` anidado y `fijar` | **Falta** |
| Salir cuando el PC llega a 0 | Sí | `RELEVO_OPT_EXIT_ON_PC_ZERO` [C] |
| Interrupción desde otro hilo, y corte (nivel 7) | `INTF.ARM`, `ARM_err` | `relevo_set_ipl`, `relevo_request_stop` (con la carrera) |
| Invalidar un tramo de código | Suma al vaciar la caché | `relevo_invalidate` [C] |
| Olvidar todo | `jit_olvidar` a mano | **Falta** en la API |
| Mapa de memoria: RAM, ROM, vacío, "avisa" | Solo la MMU; fuera, la caja muere | Páginas de 64 KB [C] |
| Acceso fuera de memoria | Mata la caja | Error de bus del 68k, a `tc_TrapCode` |
| Chips (S18) | Fallo de página entregado (no existe) | Páginas `HOST`, ya [C] |
| Presupuesto y ciclos (S18) | No cuenta ciclos [D] | Intérprete con ciclos del 68000 [C] |
| Guardar y reponer el estado entero | A mano | Copiar `relevo_regs`; falta lo de dentro (V8) |

`Registros::desde_jit` y `hacia_jit` son lo único de las bibliotecas de Rust atado al motor [C]. Pasan a la capa.

### La caja PowerPC (S17)

Su motor es el JIT PPC de Emu68, sobre el emisor de ARM (parches 0010 y 0011, unas 3200 líneas) [C]. En x86 no
hay motor. RelevoJIT es solo 68k. Opciones: QEMU empaquetado para el sistema entero (S17 ya lo prevé), o
esperar. No recomiendo un tercer JIT ahora. La interfaz de motor debe dejar sitio a "otra CPU, otro motor".

### ¿RelevoJIT también en ARM?

A favor:

- **Un solo comportamiento.** El mismo intérprete de referencia y las mismas excepciones en las dos CPU. Emu68
  tiene huecos conocidos: `FMOVEM.X` con lista dinámica no guarda nada, `FMOVE.P` trunca (bitácora de U7) [D].
- **Estabilidad.** Error de bus por tarea en vez de la muerte de la caja.
- **La caja little-endian en todas partes.** Sobran la toolchain `aarch64_be` de Bootlin, el objetivo de Rust
  big-endian con `build-std`, el E0E por proceso en el kernel y la duda de si cada CPU nueva tiene big-endian
  en EL0 (S21, S24).
- **Menos parches.** U7 sobre Emu68 son unas 3700 líneas en 280 sitios; dio tres fallos al probarlo [D].
- **S18.** Ciclos del 68000 y páginas de chips, sin tocar el kernel.
- **Las pruebas valen para los dos.** `jitdiff` no depende de la CPU: un backend nuevo se prueba desde el día 1.

En contra:

- **Madurez.** Emu68 corre software real, también en miles de PiStorm. RelevoJIT, ninguno aún.
- **Velocidad.** Emu68 saca 1,6 a 3,5 instrucciones ARM por instrucción 68k [D]. RelevoJIT paga unas 9
  instrucciones por acceso a memoria y carga y guarda registros en cada bloque [C]. Será más lento. No está
  medido; mi cálculo es entre 1,5 y 3 veces en código con mucha memoria [S].
- **Coste.** El backend x86 son 2581 líneas más 602 del emisor [C]. Uno de AArch64 sería parecido. Las banderas
  cambian: el acarreo de la resta va al revés y no hay `LAHF`.
- **Sin el backend**, el intérprete en una Pi 4 da quizá 30 o 40 millones de instrucciones por segundo [S]. Es
  un 68060. Vale para probar, no para usar.

Recomendación: **sí como meta, no todavía.**

1. Ahora: la interfaz de motor, y RelevoJIT (intérprete) como segundo motor de la caja también en ARM. Es C
   portable. Da una prueba diferencial de la caja entera: el mismo programa con los dos motores.
2. La caja de x86 con el JIT.
3. Backend de AArch64. Medir contra Emu68 con `jit-host/medidas`.
4. Cambiar el motor por defecto en ARM cuando pase las pruebas de la caja y no sea más de 1,5 veces más lento.
   Emu68 se queda mientras tanto, y para el PPC si S17 sigue.

## 4. Dash en 68k, AArch64 y x86-64

### Cómo genera código hoy [C]

Dos caminos, y cada uno hace de oráculo del otro:

- **Por C** (`compiler/codegen.py`, 30 533 líneas): genera C y llama a un compilador.
  - 68k: `m68k-amigaos-gcc`, hunks de AmigaOS. Opciones en `cflags.py`: `-O2`, `-fbbb` recortado, `--cpu`.
  - DashOS: `clang --target=aarch64-none-elf` (o `aarch64_be-none-elf` con `--be`) y `ld.lld`
    (`tools/dashos_run.py:26`). ELF estático en 0x400000 o PIE.
- **Directo** (`dashc/`, en C, 13 300 líneas): un recorrido común (`gen68k.c`) y un generador por CPU.
  `t68k_*.c` escribe hunks; `ta64_*.c` (unas 1200 líneas) escribe un objeto ELF que enlaza `ld.lld`.
- Runtime de DashOS: `runtime/dashos/plat.c`. Tres funciones de plataforma sobre `svc #0`, con el número en
  `x8`. Tiene una plataforma Linux para probar en el PC con `qemu-aarch64`.
- Estructuras del Amiga: `@amiga struct`, con la disposición del 68000 y `Ptr` de 32 bits. **En little-endian
  ya gira los bytes** de los campos de 16 y 32 bits, en los dos caminos: `_DASH_BE16` y `_DASH_BE32`
  (`codegen.py:25650`) y `rev`, `rev16` (`ta64_ops.c:841`).
- Pruebas: 558 en el A1200 emulado. En `qemu`, pasan 104 de 315 por C y 91 por el directo; el resto pide
  builtins de AmigaOS [D]. De 851 builtins, 77 compilaban para DashOS el 3 de noviembre [D].

### Qué falta para x86-64 de DashOS

| Pieza | Qué hay que hacer | Tamaño |
|---|---|---|
| Camino por C | `--target=x86_64-unknown-none-elf` en `dashos_run.py`; clang ya lo trae | Pequeño |
| `plat.c` | `_svc` con `syscall`: número en `rax`; argumentos en `rdi`, `rsi`, `rdx`, `r10`, `r8`; vuelve en `rax`, `rdx`, `r8` [D]. `_start`: la frecuencia llega en `rdi`, no hay `cntfrq_el0`. El reloj, `rdtsc` | Pequeño |
| PIE | `_dash_relocate` con `R_X86_64_RELATIVE` | Pequeño |
| Pruebas en el PC | La plataforma Linux corre nativa, sin `qemu`: más rápido que en ARM | Pequeño |
| Camino directo | Un `tx64_*.c`. El modelo (acumulador, pila, variables en registros) encaja en x86. Hoy pide 9 registros para variables (`MAX_REGS`); x86 da 5 o 6 | Medio |
| Opción | `--dashos` hoy quiere decir AArch64. Hace falta `--arch` | Pequeño |

Supuestos que hay que vigilar:

- **Tamaño de puntero.** Ya se limpió para AArch64 (`_DashP`). x86-64 es LP64 igual: no espero sorpresas [S].
- **Int devuelto en 32 bits.** En AArch64 la parte alta de `w0` queda sin definir y el runtime devuelve
  `_DashP` [D]. En x86-64 pasa lo mismo con `eax`. El mismo cuidado vale.
- **Orden de bytes.** Solo hay 2 pruebas de `__BYTE_ORDER__` en `codegen.py`, más la de `@amiga struct` [C]. x86
  es little-endian como el AArch64 nativo: lo que pasa allí, pasa aquí.
- **`peek_*` y `poke_*` van en el orden del procesador** [D]. En little-endian leen al revés la memoria del
  Amiga. Dentro de la caja es una trampa. Hacen falta variantes big-endian, o que giren dentro de la caja.
- **Direcciones fijas del runtime**: montón en 4 GB, búferes entre 1,5 y 2 GB, caja desde 1 GB (`plat.c`) [C].
  En un programa nativo valen. Dentro de la caja, los 4 GB bajos son del 68k (S28): no valen.
- **Pruebas "idénticos".** La red de seguridad compara binarios de 68k. Para x86 no hay referencia: el oráculo
  es el camino por C contra el directo, y la salida de ARM.

### Las bibliotecas Amiga dentro de la caja (el punto crítico)

Los hechos:

- En ARM las bibliotecas nativas de hoy **no son de Dash ni dependen del big-endian**: son Rust con accesos
  `from_be_bytes` detrás del rasgo `Memoria` (`caja/exec/src/memoria.rs`) [C]. Sus pruebas corren en el PC.
- Dash no tiene modo para generar una `.library` de AmigaOS: no lo encontré en `dash`, `main.py` ni `dashc`
  [S]. Tiene `@extern` para usarlas.
- Dash sí genera accesos girados en little-endian (arriba) [C].

Las tres vías:

| Vía | Compatibilidad | Estabilidad | Velocidad | Coste |
|---|---|---|---|---|
| A. Compilarlas a 68k y correrlas en el JIT | La mejor: el mismo binario corre en AmigaOS real y en el oráculo (regla 5) | Buena: el Enforcer las ve como a cualquier 68k | La peor: 2 a 5 veces más lento que nativo [S] | Modo biblioteca en Dash (tabla de saltos, romtag, argumentos en registros) |
| B. Nativas con accesos girados | Media: otro binario que el del Amiga; hay que probar aparte | Media: un puntero malo toca memoria nativa | La mejor | x86-64 en Dash; `Ptr` y `peek` girados; registros por la capa de motor |
| C. Rust con `Memoria` (lo de hoy) | Buena: accesos comprobados, pruebas en el PC | La mejor: sin `unsafe` en la lógica | La mejor | Ninguno nuevo |

Recomendación:

1. **El núcleo sigue en Rust (vía C)**: exec, dos, graphics, intuition, timer, utility. Ya existe y es portable.
   En x86 solo cambian `pegamento` (objetivo y registros) y la capa de motor.
2. **Las bibliotecas escritas en Dash, por la vía A.** Son lógica de Amiga pura: las mismas que Dash ya sabe
   hacer para AmigaOS. Un solo binario para el Amiga real, la caja de ARM y la de x86. La prueba del oráculo es
   directa. Cuando una función sea lenta, se pasa a nativa detrás de una línea A.
3. **La vía B, solo donde haga falta**: una biblioteca en Dash que hable con DashOS (canales, compositor). El
   giro ya está hecho; falta el objetivo x86-64 y la convención de registros.
4. **Las aplicaciones de Dash para DashOS son nativas** y no pasan por la caja. Les basta el camino por C.

Esto cambia una línea de `PLAN.md` ("Las bibliotecas nativas se escriben en Dash"). Es decisión del usuario.

## Para decidir el usuario

1. **¿Fusionar `revision` en `x86` antes de X7?** Recomiendo sí, ya. Y renumerar S21 y S22 de esta rama.
2. **¿Interfaz de motor en la caja, con Emu68 detrás?** Sí. Es el paso que desbloquea x86 sin romper ARM.
3. **¿RelevoJIT como único motor, también en ARM?** Sí como meta. El cambio en ARM, solo tras programas reales,
   backend de AArch64 y medida contra Emu68 (tope: 1,5 veces más lento).
4. **¿La memoria del 68k en la dirección 0 también con RelevoJIT?** Sí. Es lo que suponen la caja, Rust y Dash.
5. **¿Bibliotecas: núcleo en Rust, y las de Dash compiladas a 68k?** Sí. Cambia `PLAN.md`.
6. **¿Entregar los fallos de página al proceso?** Sí, en las dos arquitecturas, después de la caja en x86. Lo
   piden S18, V6 y un Enforcer barato. Hasta entonces, RelevoJIT comprueba cada acceso.
7. **¿PCID apagado por defecto hasta probarlo en hardware?** Sí.
8. **¿La caja PowerPC en x86?** Con QEMU empaquetado o aplazada. Sin tercer JIT por ahora.
9. **¿Dash para x86-64: primero por C?** Sí. El generador directo, después. Es trabajo de las sesiones de Dash.
10. **¿FPU con x87 (V5) antes que quitar comprobaciones (V6)?** V5 antes: no pide nada al kernel.

## Orden propuesto

| Paso | Qué | Dónde | Prioridad |
|---|---|---|---|
| 1 | Fusionar `revision` en `x86`; mitad de x86 de hilos, `esperar`, página cero; renumerar | DashOS | Estabilidad |
| 2 | TLB: no soltar memoria sin respuesta de todos los núcleos. PCID apagado por defecto | DashOS | Estabilidad |
| 3 | Capa de motor en la caja; `Registros` neutros; Emu68 detrás | DashOS (`revision`) | Compatibilidad |
| 4 | RelevoJIT: entrada anidada, base 0, vaciar todo, memoria de código del anfitrión, arreglo del aviso | RelevoJIT | Compatibilidad |
| 5 | RelevoJIT: banco de programas reales y modo en paralelo (su V2 y V3) | RelevoJIT | Compatibilidad |
| 6 | La caja con el intérprete de RelevoJIT, en ARM y en x86; diferencial contra Emu68 | DashOS | Compatibilidad |
| 7 | La caja de x86 con el JIT (X8) | DashOS | Velocidad |
| 8 | Dash para x86-64 por C; `peek` y `poke` girados; modo biblioteca 68k | Dash | Compatibilidad |
| 9 | `base/aarch64/` y `planificador.rs` partido en común y `arq` | DashOS | Estabilidad |
| 10 | Fallos de página entregados al hilo, en ARM y x86 | DashOS | Estabilidad |
| 11 | Backend de AArch64 de RelevoJIT; medir; decidir el motor de ARM | RelevoJIT | Velocidad |
| 12 | x87 (V5), sin comprobaciones (V6), registros entre bloques (V7) | RelevoJIT | Velocidad |
| 13 | IOMMU, INTx por AML y llamadas largas con pausas, antes del PC real (X9) | DashOS | Estabilidad |

Los pasos 3, 4 y 8 no dependen entre sí y los llevan sesiones distintas. El 6 necesita el 1, el 3 y el 4.

## Lo que no se miró

- No se ejecutó nada. La carrera del aviso y el riesgo de la TLB salen de leer; falta provocarlos.
- Del kernel de `revision` solo se miraron las llamadas, el permiso de ejecutar y el tratamiento de fallos.
- No se leyeron `uefi/`, el compositor, el kit, DashFS ni el sonido: S21 dice que van tal cual y no lo comprobé.
- De Dash no se leyó `codegen.py` entero (30 533 líneas): solo lo de DashOS y las estructuras del Amiga. La
  rama `dashos-a7`, sin fusionar, no se miró.
- La velocidad de RelevoJIT frente a Emu68 es un cálculo, no una medida.
