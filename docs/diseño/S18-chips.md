# S18. Los chips del Amiga

Del 2026-11-08. Lo pidió el usuario ese día. Es análisis y diseño: no hay código.

## El problema

S4 dejaba fuera los programas que tocan el chipset: juegos y demos que escriben en el Copper, el Blitter, Paula o las
CIA, y los cargadores de pista. Proponía un emulador entero, Amiberry o FS-UAE, empaquetado como aplicación.

El usuario cambió eso el 2026-11-08 (PLAN, decisiones de ese día). Los juegos y demos que tocan el chipset también son
objetivo. DashOS imita los chips con código propio, sin depender de Amiberry: "tenemos que escribir un Petunia y no
depender de un Amiberry", pero con chips. El motivo: el hardware original se estropea y no hay repuestos, y la PiStorm
tiene fecha de caducidad.

Lo que ya está decidido y condiciona el diseño:

- La CPU es el JIT de Emu68 (MPL-2.0). Si hace falta, en nuestra propia rama (PLAN, 2026-11-08).
- La caja Amiga es un proceso big-endian. Sus primeros 4 GB son el "Amiga": las direcciones 68k son direcciones
  reales del proceso (S3; `jit-host/host.c`).
- El código traducido tiene una dirección para escribir y otra para ejecutar (S3).
- Matar siempre funciona (PLAN). La caja no es una excepción.
- Nada de código de la ROM en el repositorio, y la ROM la pone el usuario (AGENTS, reglas 1 y 2).
- Cada pieza llega con su prueba del oráculo (AGENTS, regla 5).

Este documento resuelve: qué modos de caja hay, cómo se capturan los accesos a los chips con el JIT en marcha, cómo se
lleva el tiempo, cómo sale la imagen, el sonido y el disco, cómo se prueba, qué dice la sala limpia y en qué fases se
hace.

## Lo que se comprobó

En el código de Emu68 (MPL, se puede leer), rama `1_1_wip` y clon de `third_party/Emu68`:

- **Emu68 ya hace "fallo y emular" con los chips.** En la PiStorm, la zona de chips no está proyectada. Un acceso da
  un fallo de página, y `SYSPageFaultReadHandler` y `SYSPageFaultWriteHandler` (`src/aarch64/vectors.c`) decodifican
  la instrucción ARM que falló. Después llaman a `SYSReadValFromAddr` o `SYSWriteValToAddr`, que mandan el acceso al
  bus del Amiga de verdad. Ya mira a mano algunos registros: `INTENA`, `INTREQ`, el bit `OVL` de la CIA-A (proyecta
  la ROM o la RAM en la dirección 0) y la selección de unidad de la CIA-B. No reescribe la instrucción: cada acceso
  vuelve a fallar.
- **El código 68k corre en EL0 dentro de Emu68.** Los fallos que trata son de clase `0x24` (fallo de datos desde un
  nivel inferior). En DashOS ese papel lo tendría que hacer el propio proceso de la caja, no el kernel.
- **El contador de instrucciones existe y es barato.** `EMU68_INSN_COUNTER` vale 1 en `include/config.h`. El
  contador vive en `v20.d[0]` (`CTX_INSN_COUNT` en `include/M68k.h`). El traductor suma el número de instrucciones del
  bloque al salir de él (`EMIT_LocalExit` y el final de `M68K_TranslateUnit`, en `src/M68k_Translator.c`). El
  intérprete suma uno por instrucción (`InterpreterLoop`, `src/ExecutionLoop.c`).
- **Las interrupciones se miran entre bloques.** El contexto tiene un campo `INT64` con `IPL`, `ARM` y otros
  (`include/M68k.h`). El código traducido lo lee al cerrar un bucle; el intérprete, tras cada instrucción. Un bloque
  traducido puede tener hasta 256 instrucciones (`EMU68_M68K_INSN_DEPTH`), y ese tope se cambia en marcha
  (`JIT_CONTROL`, campo `JCCB_INSN_DEPTH`).
- **El intérprete no cuenta ciclos.** Ni `src/M68k_Interpreter.cpp` ni `src/M68k_INT_*.cpp` (unas 7.800 líneas)
  hablan de ciclos.

En DashOS (`kernel/src`, `docs/referencia/kernel.md`):

- **Un fallo de página mata el proceso.** `tratar` en `kernel/src/base/excepciones.rs` escribe la clase, `ELR` y
  `FAR` en la consola y llama a `terminar_actual`. No hay forma de entregarle el fallo al proceso.
- **`memoria_proyectar` solo da leer y escribir.** Falta proyectar con ejecución, que la caja necesitará de todos
  modos (S3).
- **EL0 no puede limpiar la caché de instrucciones.** `SCTLR_EL1` arranca con `0x30D00800` y luego solo se le suman
  la MMU y las cachés (`kernel/src/base/arranque.s`). El bit `UCI` queda a cero, así que `ic ivau` y `dc cvau` desde
  EL0 dan una excepción. Un JIT que reescribe su código necesita eso, o una llamada al kernel.
- **Un proceso tiene un solo hilo** y no hay llamada para crear hilos.
- **Hay temporizadores en µs** (`temporizador_crear`), que sirven para llevar el ritmo de los cuadros.
- **No hay servidor de audio** ni diseño suyo (S2 solo dice que el audio va en la clase de plazos).
- **El protocolo "archivos" existe** (`docs/referencia/servicios.md`, rama `archivos`): buscar, leer, escribir,
  listar, crear, borrar, renombrar, tamaño e info. Los atributos del Amiga (FS7) y la fecha de lo escrito están
  pendientes.

En shiva:

- FS-UAE 3.2.35 está instalado. `vasmm68k_mot` y `m68k-amigaos-as` están en `/opt/amiga/bin`.
- Hay Kickstart 3.1 de A1200 y de A500, y 3.2. No vi un Kickstart 1.3 (no busqué a fondo).
- `tools/emu_run.py --screenshot` de Dash saca la imagen con un programa dentro del Amiga (`tools/amiga/shot.c`), que
  lee la pantalla de Intuition. Eso es la memoria de la pantalla, no lo que pintan los chips: no ve el Copper.

Fuera (solo README y ficheros de licencia; ver la sección 8):

- **El núcleo de vAmiga es MPL-2.0.** Su fichero `LICENSE` dice: la aplicación, GPL-3.0 o posterior; la carpeta
  `Core` (el emulador), MPL-2.0; la CPU Moira, MIT. vAmiga emula el A500, el A1000 y el A2000.

No se comprobó nada en marcha: no hay código de esto, ni se midió el coste de un fallo.

## 1. Tres modos de caja

### Nativa (S4)

Lo de S4: exec y las bibliotecas son nativas, y no hay chips. Un acceso a `$DFF000` o a las CIA es un error: el
Enforcer lo apunta (S3). Es el modo para el Workbench, las aplicaciones y todo lo que respeta el sistema.

### Máquina entera

Para los programas que se apoderan del Amiga: arrancan de disquete o con WHDLoad, apagan el sistema y hablan con los
chips. La caja es un Amiga completo:

- **El Kickstart lo pone el usuario.** Se carga de sus ficheros en `$F80000`. Nunca va en el repositorio.
- **La memoria es la del Amiga.** Chip RAM desde 0 (512 KB, 1 MB o 2 MB), "slow RAM" en `$C00000`, RAM rápida
  donde la ponga la autoconfiguración. La CPU corre en modo supervisor, con sus vectores y sus interrupciones. Es el
  modo en que Emu68 ya corre en la PiStorm, así que es el que menos cambia el JIT.
- **Están todos los chips**: Agnus o Alice, Denise o Lisa, Paula y las dos CIA.
- **La pantalla es una ventana del escritorio** (S10), o pantalla completa. El sonido va al servidor de audio. La
  entrada llega del compositor cuando la ventana tiene el foco.
- **No usa las bibliotecas nativas.** exec, graphics y lo demás son las del Kickstart, en el JIT. Por eso no depende
  de H4 ni de H5.

También sirve para quien quiera un Amiga clásico entero, con su AmigaOS 3.x instalado, como hoy con Amiberry.

Sin Kickstart del usuario, el ROM de AROS para 68k (licencia APL, se puede repartir) podría arrancar. Usarlo como
programa no choca con la regla 1, porque no se lee su código. No se comprobó qué tal funciona con juegos.

### Mixta

Un programa que usa el sistema pero toca algún chip. Los casos más comunes, de más fácil a más difícil:

1. **Leer el joystick y el ratón a mano**: el botón en la CIA-A (`$BFE001`, bits 6 y 7), `JOY0DAT`, `JOY1DAT` y
   `POTGOR`. Es muy habitual en juegos que sí usan el sistema. Son solo lecturas, y salen de los eventos de entrada.
2. **Tocar Paula a mano** para sonar muestras.
3. **Usar el Blitter** sobre un mapa planar propio. S4 ya ofrece mapas planares que se convierten en cada cuadro.
4. **Poner su propia lista de Copper** encima de una pantalla de Intuition. Choca con la pantalla nativa: habría que
   pasar esa pantalla a la imagen de los chips. Es caro y raro.

Los chips de la máquina entera sirven tal cual para los casos 1 a 3. El 4 se deja hasta que un programa concreto lo
pida.

### Por cuál empezar

**Recomendado: la máquina entera primero.** Porque:

- Es lo que piden los juegos y las demos, que casi siempre se apoderan de la máquina.
- No necesita las bibliotecas nativas: el Kickstart trae las suyas. Se puede empezar ya, en paralelo con H4 y H5.
- Se compara con FS-UAE con el mismo Kickstart y el mismo disco, sin capas en medio.
- Es el modo natural de Emu68.
- Los chips que salen de aquí son los mismos que usará el modo mixto.

La caja nativa debe dejar libre desde ya el mapa del hardware del Amiga: `$A00000`-`$BFFFFF` (CIA), `$C00000`-`$DFFFFF`
(chips, reloj, IDE), `$E80000` (autoconfiguración) y `$F80000`-`$FFFFFF` (ROM). Así el modo mixto no pide mover nada
después. Es casi gratis: S3 ya da 4 GB a la caja.

## 2. Capturar los accesos

### El mapa

En la máquina entera, todo lo que es un aparato queda **sin proyectar**: las CIA, los registros de los chips en
`$DFF000`, el reloj (`$DC0000`), el IDE del A1200 (`$DA0000`) y la autoconfiguración (`$E80000`). La ROM se proyecta de
solo lectura. La chip RAM, la slow RAM y la RAM rápida son memoria normal, compartida con los chips del mismo proceso.

Un acceso de la CPU a la chip RAM no se captura. Es memoria, y los chips la leen cuando les toca (sección 3).

### El primer acceso: un fallo

1. El código traducido hace un `ldr` o un `str` a `$DFF180`. La página no está: fallo.
2. El kernel no mata el proceso: le entrega el fallo al propio hilo (abajo, "Lo que pide al kernel").
3. El manejador de la caja decodifica la instrucción ARM, como ya hace Emu68 en `vectors.c`. Saca el tamaño, el
   registro del dato, la dirección y si el registro base se actualiza (`(An)+` y `-(An)`).
4. Llama al chip que toca, pone el resultado en el registro y sigue tras la instrucción.

### Los siguientes: la instrucción reescrita

Es lo que hace Dolphin con su "fastmem" ("backpatching"):

- Tras el primer fallo, el manejador genera un trampolín para ese sitio y cambia la instrucción que falló por un `bl`
  al trampolín. Escribe por la dirección de escritura del código y limpia la caché de instrucciones.
- El trampolín guarda lo que el JIT necesita, llama al manejador del chip y repone el registro. Ya no hay fallo.
- Si la misma instrucción unas veces toca chips y otras memoria (una rutina de copia, por ejemplo), el trampolín mira
  la dirección y hace el acceso normal cuando no es un aparato.
- El trampolín lleva como dato fijo **cuántas instrucciones van en el bloque** hasta ese acceso. Así el manejador sabe
  la hora exacta del acceso (sección 3).
- Los trampolines van en una zona del código traducido a menos de 128 MB del bloque, el alcance de un `bl`. Si se
  vacía la caché de traducciones, se van con ella.

### Direcciones conocidas al traducir

`move.w d0,$DFF180` lleva la dirección dentro de la instrucción. El JIT la ve al traducir y puede emitir la llamada
directa, sin fallo y sin trampolín. Es un cambio en el traductor de Emu68 (MPL, en nuestra rama). Las demos y juegos
suelen usar `lea $DFF000,a6` y luego `$180(a6)`: eso no se ve al traducir y va por el fallo y la reescritura.

### Casos que hay que cuidar

- **`move.l` y `movem`** a los chips escriben dos o más registros de 16 bits seguidos. El trampolín los separa y los
  aplica en orden. Emu68 traduce `movem` con `ldp` y `stp`; el decodificador ya los conoce.
- **Accesos de un byte** a registros de 16 bits. En el Amiga dan resultados raros. Se imita lo documentado; lo demás
  se apunta.
- **Direcciones de 24 bits.** El 68000 y el 68EC020 ignoran el byte alto de la dirección, y hay programas que guardan
  marcas ahí. El JIT usa la dirección tal cual, así que eso cae fuera de los 16 MB: falla, el manejador quita el byte
  alto y reescribe el sitio con un trampolín que lo quita siempre. Repetir los 16 MB 256 veces en las tablas de páginas
  también valdría (unos 8 MB de tablas), pero es más caro.
- **Código que llega por DMA.** Los cargadores de pista leen código con el DMA del disco o lo copian con el Blitter, y
  luego saltan a él. El JIT tiene que tirar las traducciones de ese tramo. Los chips saben qué escribieron, así que
  avisan al JIT. El código que se modifica a sí mismo con la CPU ya es un problema de Emu68; no lo estudié.
- **El bit `OVL`** de la CIA-A pone la ROM en la dirección 0 al arrancar. Se cambia la proyección de esa zona, como
  hace Emu68 con `mmu_map`. Pasa una vez por arranque.
- **Registros del JIT.** El manejador corre en el mismo hilo que el JIT. No puede tocar los registros ARM ni los
  vectores (`v20` lleva el contador, por ejemplo) que el JIT tiene vivos sin guardarlos.

### Lo que pide al kernel

1. **Entregar el fallo al proceso.** Una llamada nueva, por ejemplo `excepcion_manejador(dirección, pila)`. Con ella,
   un fallo de datos, de instrucción, de alineación o una instrucción ilegal en EL0 no mata el proceso:
   - El kernel escribe el marco (x0 a x30, `SP`, `PC`, `NZCV`, `ESR`, `FAR`) en la pila del manejador, con accesos sin
     privilegio como en C31.
   - Vuelve a EL0 en el manejador, con x0 apuntando al marco.
   - El manejador vuelve con una llamada `reanudar(marco)`, que repone todo. Hace falta porque los 31 registros están
     en uso: no queda ninguno libre para el salto final.
   - Un fallo dentro del manejador termina el proceso, como hoy. Así no hay bucles.
   - Es en el mismo hilo, no por un canal hacia otro hilo, como en Fuchsia. Por un canal serían dos cambios de hilo
     por fallo.
2. **Proyectar con ejecución**, con un alias de escritura y otro de ejecución para el código traducido (S3).
3. **Limpiar la caché de instrucciones desde EL0** (`SCTLR_EL1.UCI`), o una llamada que lo haga. Lo necesita
   cualquier JIT, no solo este.
4. **Hilos**, para pasar trabajo a otro núcleo (sección 3). S4 también los necesita.

El punto 1 sirve además para el Enforcer de S3, que hoy solo podría matar la caja, y para las señales de la caja
Linux (S7).

### Coste de un fallo

Sin medir. Es un cálculo:

- Entrar y salir de EL1 cuesta del orden de un centenar de ciclos cada vez en una Cortex-A72 (dato general, no medido
  aquí).
- Con el marco, la decodificación, el chip y `reanudar` (otra entrada al kernel), calculo entre 0,5 y 2 µs por fallo
  en la Pi 4.
- Con la instrucción ya reescrita, una llamada al trampolín debería costar entre 10 y 50 ns.

Un supuesto: un juego que hace 200 operaciones del Blitter por cuadro escribe unos 2.000 registros por cuadro. Con un
fallo en cada acceso serían unos 2 ms de cada 20 ms, el 10 % de un núcleo. Con la reescritura, menos de 0,1 ms. Por
eso la reescritura no es opcional. El coste del primer fallo se paga una vez por sitio.

Se mide en la Pi 4 real. En QEMU no vale para tiempos.

## 3. El tiempo

### El problema

En el Amiga, la CPU y los chips comparten un reloj. Una demo cambia un color "cuando el haz va por la línea 100". Un
juego espera al barrido vertical. Una canción lleva el ritmo con un temporizador de la CIA. El JIT no cuenta ciclos:
ejecuta tan deprisa como puede.

### Un reloj virtual

**La hora de la caja sale de lo que ha ejecutado la CPU**, no del reloj de la placa:

- Cada instrucción traducida suma su coste estimado en ciclos de 68000. El traductor lo saca de una tabla por tipo de
  instrucción, al traducir. En marcha cuesta lo mismo que contar instrucciones: una suma al salir del bloque. Hoy suma
  instrucciones, no ciclos, y en un inmediato de 12 bits; hay que ampliarlo.
- **Un contador hacia atrás hasta el próximo suceso**, como el "downcount" de Dolphin. Al salir de cada bloque se
  resta el coste. Si pasa de cero, el JIT sale al planificador de los chips. Son una o dos instrucciones por bloque.
- **Un acceso a un chip sabe su hora exacta**: la del principio del bloque más lo que lleva el bloque hasta ese
  acceso, que el trampolín lleva como dato fijo.
- **Los chips van con pereza.** No corren solos. Se ponen al día cuando la CPU lee o escribe uno de sus registros, y
  en sus sucesos.

Con esto, el mismo programa y la misma entrada dan siempre el mismo resultado. Es lo que permite compararlo con FS-UAE
y repetir un fallo.

### Los sucesos

El planificador de los chips tiene una cola de sucesos con su hora:

| Suceso | Cada cuánto (PAL) | Qué hace |
|---|---|---|
| Fin de línea | 64 µs (227,5 ciclos de color) | Denise pinta la línea; Paula genera sus muestras; el Copper avanza |
| Fin de cuadro | 20 ms (312 o 313 líneas) | Interrupción `VERTB`; se entrega la imagen; se espera al reloj real |
| Fin de una operación del Blitter | Según su tamaño | Baja `BBUSY`, interrupción `BLIT` |
| Temporizadores de las CIA | Según lo programado (reloj E, 709 kHz) | Interrupción de la CIA |
| Fin de un bloque de audio | Según periodo y largo | Interrupción `AUDx` |
| Fin de un bloque de disco | Según la longitud | Interrupción `DSKBLK` |

Las cifras PAL son las del Hardware Reference Manual, de memoria; hay que comprobarlas con el manual. En NTSC
cambian.

Las interrupciones de los chips ponen el nivel en el campo `IPL` del contexto del JIT, que ya existe.

### La exactitud de las interrupciones

El JIT mira las interrupciones entre bloques. Un bloque largo retrasa la interrupción hasta su final. A la velocidad de
un A500, 256 instrucciones son unas cinco líneas de pantalla. En la máquina entera se baja el tope del bloque (por
ejemplo a 32), que ya se cambia en marcha. Eso cuesta algo de velocidad y no se ha medido cuánto.

### Escrituras con su hora

Una escritura en un registro de color a mitad de línea tiene que verse a mitad de línea. Cada escritura de la CPU o
del Copper en un registro de Denise se guarda con su posición horizontal en la lista de cambios de la línea. Denise
pinta la línea por tramos entre cambios. Así salen los efectos de Copper a mitad de línea, como los "copper chunky"
que cambian un color cada 8 píxeles.

### Niveles de exactitud

| Nivel | Qué imita | Qué programas cubre (estimado, sin medir) |
|---|---|---|
| Cuadro | Un estado de los chips por cuadro | Pantallas sin efectos de haz. Solo sirve como primer paso |
| Línea | Todo por línea, con cambios a mitad de línea con su hora; Blitter con duración aproximada | La mayoría de los juegos y muchas demos: barras de Copper, pantallas partidas, sprites reutilizados, desplazamiento por línea |
| Ciclo | El reparto del bus entre la CPU y el DMA, el Blitter palabra a palabra, la CPU con sus ciclos exactos | Demos que sincronizan la CPU con el haz al ciclo, protecciones y cargadores muy justos, juegos con carreras que solo salen bien a la cadencia del 68000 |

**Recomendado: el nivel línea como objetivo.** El nivel ciclo, como fase opcional al final. Necesita otra CPU, porque
el JIT no puede ser exacto al ciclo (ver Moira en la sección 7).

### Límite de velocidad

Muchos juegos esperan un 68000 a 7 MHz: bucles de espera con `dbf`, el saludo con el teclado, la música atada a la
CPU. Con el reloj virtual, la velocidad es un perfil:

- **A500**: 68000 a 7,09 MHz, con la tabla de ciclos del 68000.
- **A1200**: 68020 a 14 MHz.
- **Rápido**: un múltiplo configurable, para lo que va mejor con una CPU rápida.

En cada cuadro, la CPU gasta los ciclos del perfil. Al llegar al fin de cuadro, el hilo duerme hasta que pasan los
20 ms reales (`temporizador_crear`). Si el JIT no llega a tiempo, el cuadro se alarga; a la velocidad de un A500 no
debería pasar.

Muchos fallos con CPU rápida ya los arregla WHDLoad con sus parches: esperas al Blitter, bucles de retardo, el
teclado. Con el perfil A500 hacen menos falta.

### Esperas activas

Los juegos esperan al barrido vertical leyendo `VPOSR` o `INTREQR` en un bucle. Cada vuelta es una llamada al chip y
gasta pocos ciclos virtuales: muchas vueltas para nada. Si la misma instrucción lee el mismo registro sin que nada
cambie entre medias, la hora virtual salta al próximo suceso. Es lo que Dolphin llama "idle skipping". Ahorra CPU y
energía.

### ¿Los chips en otro núcleo?

La caja usa un núcleo a la vez para el 68k (S4, el testigo). Los chips no son tareas 68k, así que podrían ir en otro
núcleo. Pero muchas lecturas dependen del estado de los chips "ahora": `BBUSY`, `VPOSR`, `INTREQR`, `CLXDAT`, los
temporizadores. Cada una obligaría a esperar a que el otro núcleo llegue a la hora de la CPU.

**Recomendado: partir el trabajo así.**

- **En el hilo de la CPU**, las máquinas de estado: haz, Copper, Blitter, CIA, interrupciones, DMA. Son contadores y
  sucesos, baratos y deterministas.
- **En otro hilo**, el trabajo de píxeles y de sonido. Al acabar cada línea, el hilo de la CPU copia lo que Denise
  necesita: los datos de los planos de esa línea, los sprites y la lista de cambios. Es menos de 1 KB por línea, unos
  15 MB/s. El otro hilo convierte a píxeles y mezcla el audio sin compartir nada vivo.
- `CLXDAT` (choques de sprites) se calcula en el hilo de píxeles. Leerlo obliga a esperar a que ese hilo llegue a la
  línea actual. Pocos juegos lo leen.

Primero todo en un hilo. El reparto llega cuando el kernel tenga hilos y haya medidas.

### Lo que se sacrifica en el nivel línea

- **Carreras con la chip RAM.** Denise lee los planos al acabar la línea en hora virtual. Una escritura de la CPU que
  en el Amiga llegaría justo antes o después del haz puede caer al otro lado. Casi todos los juegos usan doble búfer y
  no lo notan.
- **El Blitter acaba de golpe.** Se hace entera al escribir `BLTSIZE`, y `BBUSY` sigue a uno el tiempo que tardaría.
  Un programa que espera al Blitter va bien. Uno que trabaja en paralelo con él sobre los mismos datos, no.
- **El robo de ciclos.** En el Amiga, seis planos en baja resolución quitan ciclos a la CPU. Aquí se aproxima con un
  coste medio por línea.

## 4. Los chips, uno a uno

| Chip | Qué hace | Cómo se imita (nivel línea) |
|---|---|---|
| Agnus / Alice | Haz, DMA, Copper, Blitter, interrupciones, tamaño de la chip RAM | Haz y `VPOSR` calculados de la hora virtual. Copper ejecutado con pereza hasta la hora pedida, contra la posición del haz. Blitter entero al escribir `BLTSIZE`: copia, relleno y líneas. Interrupciones a `IPL`. ECS: `BEAMCON0` y 1 o 2 MB de chip. AGA: los modos de lectura (`FMODE`) |
| Denise / Lisa | Planos a píxeles, sprites, prioridades, choques | Por línea y por tramos de cambios. Baja y alta resolución, entrelazado, dos campos, desplazamiento fino, HAM y EHB. Ocho sprites, sprites unidos, prioridades de `BPLCON2`. AGA: 256 colores de 24 bits, HAM8, superalta resolución y sprites anchos |
| Paula | Audio, interrupciones, disco, puertos de mandos analógicos | Cuatro canales con periodo, volumen y largo, leídos de la chip RAM al generar. Filtro de paso bajo aproximado (el del A500 y el del LED). Interrupciones `AUDx`. Disco: ver la sección 5 |
| CIA-A y CIA-B | Temporizadores, reloj de cuadros y de líneas, teclado, puertos del disco, botones | Temporizadores y TOD calculados de la hora virtual; los desbordes, sucesos. Teclado por el registro serie con su saludo. Líneas del disco (motor, paso, cara, índice). `OVL` |

Los chips leen la chip RAM directamente: está en el mismo proceso. La caja es big-endian, como el Amiga, así que la
chip RAM se lee sin dar la vuelta a los bytes. En el modo de U7 (CPU sin big-endian) sí habría que darla.

## 5. Salidas

### La pantalla

- **Una superficie del compositor** (S9): un objeto de memoria compartido, con doble búfer.
- **De planos a píxeles con NEON.** Es la conversión inversa del "c2p" de los Amiga con tarjeta. Cálculo a ojo: unos
  1.000 a 2.000 instrucciones por línea de 320 píxeles con 6 planos, unos 0,3 ms por cuadro en una A72. Sin medir.
- **El tamaño** es el área visible con márgenes ("overscan") de PAL. El compositor escala.
- **La misma conversión sirve a la caja nativa**, para los mapas planares de S4 (sección 3 de S4).
- **50 Hz en una pantalla de 60 Hz** da tirones: un cuadro de cada cinco se repite. A pantalla completa, lo mejor es
  pedir a la salida HDMI un modo de 50 Hz (fase B de S9). En ventana se acepta.

### El sonido

Paula genera sus muestras en hora virtual, al ritmo de las líneas. Se pasan a 48 kHz y se mandan al servidor de audio.
Como el ritmo de los cuadros y el reloj del sonido derivan, se reajusta la frecuencia un poco, como hace cualquier
emulador. **Falta el servidor de audio** de DashOS: es una dependencia.

### El disquete

- **ADF**: el disco en sectores de AmigaDOS. El formato de las pistas de AmigaDOS (MFM, palabra de sincronía,
  cabeceras) está documentado. La caja codifica cada pista en MFM al leerla y la da por el DMA del disco
  (`DSKPTH`, `DSKLEN`, `DSKSYNC`, `ADKCON`).
- **Discos con formato propio** (protecciones, cargadores con pistas raras) no caben en un ADF. Necesitan imágenes en
  bruto: el ADF ampliado o el IPF. El IPF se lee con una biblioteca de SPS con licencia propia (sin revisar). Va
  después.
- **Velocidad**: real por defecto, porque hay cargadores que cuentan con ella. Rápida como opción.
- El disco escribe en una copia, no en el fichero original, salvo que el usuario lo pida.

### El disco duro de la caja

WHDLoad necesita un AmigaOS que arranque de un disco duro. Dos caminos:

- **Una placa de autoconfiguración propia** con una ROM propia (68k, escrita desde cero) que monta un gestor de
  ficheros de AmigaDOS. El gestor recibe los paquetes de DOS (`ACTION_READ` y demás, documentados en el NDK) y los pasa
  a lo nativo con la línea A de S4, que también funciona en la máquina entera. Lo nativo habla el protocolo "archivos"
  (`servicios.md`). Un cajón de DashOS se ve como un volumen del Amiga.
- **Un HDF por el IDE del A1200** (Gayle), para quien tenga discos hechos en otro emulador.

**Recomendado: la placa propia primero.** Comparte los ficheros con DashOS. Le faltan al protocolo los atributos del
Amiga y los comentarios (FS7) y la fecha (no hay reloj de la hora).

### Teclado, ratón y mandos

- **Teclado**: el compositor manda los eventos a la ventana con el foco (`servicios.md`, USB). La caja traduce el uso
  HID al código de tecla del Amiga y lo manda por el registro serie de la CIA-A, con su saludo.
- **Ratón**: contadores de `JOY0DAT`, botón izquierdo en la CIA-A, derecho en `POTGOR`.
- **Joystick**: `JOY1DAT` y el botón en la CIA-A. Del teclado (cursores) o de un mando USB, que hoy no tiene
  controlador: el servicio USB solo lleva teclado y ratón en modo de arranque.

### Guardar el estado

Toda la máquina está en un proceso: chip RAM, registros, chips y la CPU. Guardar y cargar el estado es copiar eso; las
traducciones del JIT se tiran. Encaja con "congelar un programa y llevárselo a otra máquina" de las propuestas de la
visión (PLAN, 2026-11-07).

## 6. Pruebas

### El oráculo

FS-UAE con el mismo Kickstart, el mismo disco y el perfil más parecido (A500 o A1200, sin JIT), como `qemu-ppc` en el
banco PPC de S17. Lo que se compara:

| Qué | Cómo | Exactitud |
|---|---|---|
| Blitter (copia, relleno, líneas, máscaras) | El programa hace la operación y escribe el resultado o su suma | Exacta, byte a byte. No depende del tiempo |
| CIA y haz | El programa apunta el orden de sucesos y lecturas de `VPOSR` y temporizadores | Con margen: el nivel línea no es exacto al ciclo |
| Imagen | Captura del cuadro N en los dos | Exacta en pantallas quietas; con margen en efectos de haz |
| Sonido | Grabación de los dos | Con margen: el filtro difiere. Mejor comparar sin filtro |
| Arranque | Kickstart hasta "inserta un disco", un ADF propio que arranca | Imagen y salida |

**Falta resolver cómo sacar de FS-UAE la imagen que pintan los chips** (no la memoria de la pantalla, que es lo que
saca `shot.c`) y el sonido, sin ventana. No lo comprobé. Caminos: su función de captura, o grabar vídeo.

### Programas de prueba

- **Propios, en ensamblador** (`vasmm68k_mot`). Se apoderan de la máquina, tocan un chip, apuntan el resultado y
  acaban. Van en ADF de arranque o se lanzan desde un disco duro. Son del repositorio.
- **Por fases**: cada fase de la sección 10 llega con los suyos.
- **El banco antes que la placa.** Como F1 de S17, los chips se desarrollan primero en un programa Linux big-endian
  junto a `jit-host/`, con `qemu-aarch64_be` en shiva. Allí el fallo llega como señal (`SIGSEGV`), la imagen sale en
  PNG y el sonido en WAV. No espera al kernel.

### Juegos y demos

Los del usuario, **solo en local, nunca en el repositorio**. Una lista fija de juegos WHDLoad y de demos, con una tabla
de "arranca / se juega / fallos" y capturas en momentos fijos, comparadas con FS-UAE. En el repositorio solo va la
lista de nombres y los resultados.

## 7. Sala limpia y licencias

### Lo que hay

| Fuente | Licencia | Para qué |
|---|---|---|
| Hardware Reference Manual (Commodore, 3.ª edición) | Libro con copyright; leerlo no lo copia | La fuente principal: registros, tiempos, DMA, formatos |
| AGA | Sin manual oficial completo, que yo sepa | Documentos de la comunidad (sin revisar cuáles ni su licencia) |
| FS-UAE como programa | GPL, se usa tal cual | Oráculo de caja negra |
| UAE, WinUAE, Amiberry, FS-UAE (su código) | GPL-2.0 | No se lee (encargo) |
| vAmiga, carpeta `Core` | MPL-2.0 (según su `LICENSE`) | Se puede leer y, con su aviso MPL, reutilizar |
| Moira (CPU 68000 de vAmiga) | MIT | Candidata para la CPU del nivel ciclo |
| Emu68 | MPL-2.0 | El JIT; los cambios en sus ficheros siguen siendo MPL |
| EMU68AGA Bare Metal | GPL-2.0 o posterior en lo suyo, y dice que integra código de UAE y WinUAE | No se lee |
| Bellatrix | Sin licencia publicada | No se lee ni se usa: sin licencia, todos los derechos reservados |

### Lo que propongo

**Para UAE, WinUAE y Amiberry, en este orden:**

1. **Primero, manual y caja negra.** El Hardware Reference Manual y pruebas propias en FS-UAE. Cubre el nivel cuadro y
   casi todo el nivel línea.
2. **Después, vAmiga como referencia.** Es MPL como DashOS: se puede leer sin sala limpia. Antes, una sesión revisa
   que los ficheros de `Core` llevan la cabecera MPL y que no vienen de UAE. Solo leí el `LICENSE`.
3. **Solo si un comportamiento concreto no sale de lo anterior**, una sesión de analista lee UAE o WinUAE y escribe
   una especificación en prosa en `docs/spec/hw/amiga-*.md`. Sin pseudocódigo ni nombres de sus variables. Quien
   implementa no lo lee. Es la regla 1 tal como está para Linux. Habría que ampliarla a UAE en `AGENTS.md`.

El conocimiento fino del hardware que hay en WinUAE salió de años de pruebas con máquinas reales. Para el nivel ciclo
quizá haga falta el paso 3. Para el nivel línea, creo que no.

### Reutilizar vAmiga o escribir los chips

vAmiga emula A500, A1000 y A2000, es decir OCS y ECS. No vi que emule AGA. Está pensado para ser exacto al ciclo junto
a una CPU que cuenta ciclos (Moira). DashOS necesita chips con pereza, atados al reloj virtual del JIT. Adaptar su
planificador de sucesos podría costar tanto como escribirlo. No leí su código, así que no lo sé.

**Recomendado: chips propios, pensados para el JIT, con vAmiga como referencia de lectura** (tras revisar las
cabeceras). Si al leerlo se ve que su Blitter o su Denise se pueden tomar casi tal cual, se toman con su aviso MPL.

## 8. Proyectos parecidos

Mirados solo por su README y su licencia, el 2026-11-08.

| Proyecto | Qué es | Licencia | En qué se distingue DashOS |
|---|---|---|---|
| EMU68AGA Bare Metal (futureretrofusion) | La Pi 3 se convierte en un Amiga AGA, sin sistema debajo. CPU con Emu68; Alice, Lisa, Paula, Copper, Blitter y CIA virtuales. Versión 0.10.40a, del 23-10-2026. Su README dice que la compatibilidad con WHDLoad varía | GPL-2.0 o posterior en lo suyo; integra Emu68, AGA-PiStorm, PiStorm y UAE/WinUAE | La placa es solo un Amiga. En DashOS el Amiga es un proceso de un sistema moderno: una ventana más, que se mata, con varias a la vez. Chips propios con licencia MPL, reloj virtual determinista y probado contra FS-UAE. Y además la caja nativa para lo que respeta el sistema |
| Bellatrix (JJDSNT) | AROS de 68k sobre Emu68 en la Pi 3, sin sistema debajo, hasta el escritorio. Sin chips: "Nothing here emulates a machine". Paula, Denise y las CIA, más adelante, con otro proyecto (Rigel). Su README dice que lo ha generado una IA | Sin licencia publicada | Corre un AmigaOS entero (AROS) en el JIT, sin bibliotecas nativas. DashOS lleva las bibliotecas a nativo (S4) y los chips para lo que no respeta el sistema |
| Emu68 en la PiStorm | La CPU de un Amiga real, con los chips de verdad | MPL-2.0 | Sin hardware Amiga. Es la razón del plan: la PiStorm caducará |
| vAmiga | Emulador de A500, A1000 y A2000 para macOS | Aplicación GPL-3.0; núcleo MPL-2.0; CPU MIT | Intérprete exacto al ciclo en un PC. DashOS busca un JIT en ARM con chips por línea |

## 9. Riesgos

| Riesgo | Dónde se ve | Qué se haría |
|---|---|---|
| El nivel línea no basta para muchos juegos | Fases 4 y 5, con la lista de juegos | Afinar los casos (Blitter con duración real, robo de ciclos); el nivel ciclo como fase opcional |
| La reescritura de instrucciones choca con cómo Emu68 cachea y verifica sus bloques | Fase 1 | Estudiar `M68K_VerifyUnit` y la caché de traducciones antes; hacerlo en nuestra rama |
| Cambiar el traductor (ciclos, contador hacia atrás, llamadas directas) aleja nuestra rama de Emu68 | Fase 1 | Cambios pequeños y aislados; mandarlos como PR si Michal los quiere (PLAN, 2026-11-08) |
| El coste de un fallo es mayor de lo calculado | Fase 1, en la Pi 4 | Con la reescritura se paga una vez por sitio; si no basta, más direcciones resueltas al traducir |
| Bloques largos retrasan las interrupciones | Fases 2 y 3 | Bajar el tope de instrucciones por bloque en la máquina entera |
| Sin servidor de audio ni mandos USB | Fase 4 | Son dependencias: el audio de H6 y un controlador HID de mandos |
| No sale de FS-UAE la imagen de los chips sin ventana | Fase 2 | Probar su captura; si no, grabar vídeo y sacar cuadros |
| AGA mal documentado | Fase 6 | Caja negra en FS-UAE con el Kickstart del A1200; vAmiga no ayuda ahí |
| Discos protegidos | Fase 4 en adelante | Primero ADF y WHDLoad; IPF tras revisar su licencia |
| Pi 3 y THEA500 Mini (Cortex-A53) más lentas | Fase 6 | Medir; el nivel línea en OCS debería ir, AGA quizá no |

## 10. Fases y tamaño

Las fases 1 a 3 van en el banco (Linux big-endian, junto a `jit-host/`), sin esperar al kernel. Las demás necesitan
la caja en DashOS: entrega del fallo, ejecución, hilos, superficie del compositor, audio y entrada.

| Fase | Qué | Se enseña (la prueba) | Tamaño (estimado, sin medir) |
|---|---|---|---|
| 1 | Captura de accesos: fallo, decodificación, reescritura, llamadas directas al traducir, 24 bits. Reloj virtual con contador hacia atrás. Las dos CIA | Programas propios que leen temporizadores y `VPOSR`: mismo orden de sucesos que en FS-UAE. Coste por acceso medido | 2.000 a 3.000 líneas, parte en el traductor de Emu68 |
| 2 | Máquina entera mínima: memoria del Amiga, `OVL`, haz e interrupciones, Copper, planos básicos, Blitter entero (copia, relleno, líneas), unidades de disco sin disco | El Kickstart 3.1 de A500 del usuario hasta la pantalla de "inserta un disco", igual que en FS-UAE | 6.000 a 9.000 líneas |
| 3 | Denise completa (OCS y ECS): resoluciones, entrelazado, dos campos, HAM, EHB, desplazamiento, sprites, choques, cambios a mitad de línea | Programas propios con barras de Copper, sprites y modos: la misma imagen que FS-UAE | 4.000 a 6.000 líneas |
| 4 | Paula (audio e interrupciones), disco ADF por DMA, teclado, ratón y joystick. Ya en DashOS: ventana, audio y entrada | Un ADF propio que arranca y suena; un juego del usuario jugable (en local) | 4.000 a 6.000 líneas |
| 5 | Disco duro: placa de autoconfiguración, ROM y gestor de DOS sobre "archivos". WHDLoad | El Workbench 3.1 del usuario y una lista de juegos WHDLoad, con su tabla de resultados | 2.000 a 4.000 líneas (68k y nativo) |
| 6 | AGA: Alice y Lisa | El Kickstart del A1200 al Workbench; programas AGA propios contra FS-UAE | 3.000 a 5.000 líneas |
| 7 | Modo mixto: entrada, Paula y Blitter para la caja nativa | Un juego que usa el sistema y lee el joystick a mano | 1.000 a 2.000 líneas |
| 8 (opcional) | Nivel ciclo: CPU exacta (Moira u otra), reparto del bus | Una demo que hoy falla y en FS-UAE va | 8.000 a 15.000 líneas |

Las fases 1 a 7 suman entre 22.000 y 35.000 líneas. Con el ritmo de la caja 68k como referencia, las fases 1 a 3 son
del orden de meses de una línea de trabajo. Son estimaciones sin medir: no se ha escrito nada.

## Recomendación, resumida

1. **Tres modos**: nativa (S4), máquina entera y mixta. **Se empieza por la máquina entera.** La caja nativa deja
   libre desde ya el mapa del hardware del Amiga.
2. **Captura**: páginas sin proyectar, fallo entregado al propio proceso, y la instrucción reescrita para que llame
   directo al chip. Las direcciones absolutas, resueltas al traducir.
3. **Al kernel le hace falta**: entregar el fallo al proceso, proyectar con ejecución, limpiar la caché desde EL0 e
   hilos. Lo primero sirve también al Enforcer y a la caja Linux.
4. **Tiempo**: reloj virtual con los ciclos estimados de cada instrucción, contador hacia atrás hasta el próximo
   suceso, chips con pereza. Determinista y comparable con FS-UAE.
5. **Exactitud**: nivel línea, con cambios a mitad de línea. El nivel ciclo, opcional al final.
6. **Velocidad**: perfiles A500, A1200 y rápido; el hilo duerme al acabar el cuadro.
7. **Núcleos**: las máquinas de estado en el hilo de la CPU; los píxeles y el sonido en otro hilo, con copias por
   línea.
8. **Disco duro**: placa propia que habla "archivos" por la línea A.
9. **Sala limpia**: manual y caja negra primero, vAmiga (MPL) como referencia después, UAE solo con analista y en
   prosa si hace falta.
10. **Las fases 1 a 3, ya, en el banco**, sin esperar al kernel.

## Lo que queda abierto

Decidido por el usuario el 2026-11-08: todas como se recomienda ("vale a todo").

1. ¿Se empieza por la máquina entera, antes que el modo mixto? Recomendado: sí.
2. ¿Nivel línea como objetivo y el nivel ciclo solo como fase opcional? Recomendado: sí.
3. ¿Se añade al kernel la entrega del fallo al propio proceso (con `reanudar`), en el mismo hilo? Recomendado: sí, como
   tarea C antes de la fase 4. También la pide el Enforcer.
4. ¿Se puede leer vAmiga (`Core`, MPL-2.0) y reutilizar trozos con su aviso? Recomendado: sí, tras revisar sus
   cabeceras y su origen.
5. ¿Puede una sesión de analista leer UAE o WinUAE y escribir especificaciones en prosa, como con Linux? Recomendado:
   no de entrada; sí para un comportamiento concreto que no salga del manual, de vAmiga ni de las pruebas, ampliando
   la regla 1 de `AGENTS.md`.
6. ¿Perfil de velocidad por defecto de la máquina entera? Recomendado: A500 para OCS y ECS, A1200 para AGA, con
   "rápido" a mano.
7. ¿Las fases 1 a 3 en el banco ya, en paralelo con H4 y H5? Recomendado: sí, si hay manos libres.
8. ¿El ROM de AROS como Kickstart libre para quien no tenga uno? Recomendado: probarlo en la fase 2 como extra; el
   oráculo sigue siendo el Kickstart real.

## Cómo encaja

- Con S4: cambia su sección 7 y su pregunta 3. Lo que toca el chipset ya no va a Amiberry, va a la máquina entera. La
  línea A de S4 sirve también para el disco duro de la máquina entera. La conversión de planos a píxeles sirve a los
  mapas planares de S4.
- Con S3: el hueco de 4 GB acoge el mapa del Amiga; la entrega del fallo hace posible el Enforcer sin matar la caja;
  el código traducido sigue con sus dos direcciones.
- Con S2: la caja de chips va en la clase justa; el audio, en la de plazos. Los cuadros se pasan con temporizadores.
- Con S9 y S10: la pantalla de la máquina es una superficie más, en ventana o a pantalla completa; a pantalla
  completa, mejor un modo de 50 Hz.
- Con S6: la máquina entera solo ve su ventana, su audio, su entrada y los ficheros que se le dan (ADF, cajones).
- Con S16: la llamada nueva del kernel cumple sus reglas: el marco se escribe con accesos sin privilegio y un fallo en
  el manejador termina el proceso.
- Con S17: el mismo banco y el mismo tipo de oráculo (`qemu-ppc` allí, FS-UAE aquí).
- Con U7: sin big-endian, los chips tienen que dar la vuelta a los bytes al leer la chip RAM, y el manejador al tomar
  el valor del registro.
- Con S15: las fases 1 a 3 van antes de H4 en el banco; la 4 y siguientes, después de H5 y con el sonido de H6.

## Fuentes

- Emu68, rama `1_1_wip` (en el scratchpad, solo lectura) y `third_party/Emu68`: `src/aarch64/vectors.c`
  (`SYSHandler`, `SYSPageFault*Handler`, `SYS*ValFromAddr`, `SYSValidateUnit`), `src/M68k_Translator.c`
  (`EMIT_LocalExit`, contador), `src/ExecutionLoop.c` (`InterpreterLoop`), `include/M68k.h` (`INT64`, `INSN_COUNT`,
  `JCCB_INSN_DEPTH`), `include/config.h`.
- DashOS: `kernel/src/base/excepciones.rs`, `kernel/src/base/arranque.s`, `docs/referencia/kernel.md`,
  `docs/referencia/servicios.md` (rama `archivos`), `jit-host/host.c`.
- EMU68AGA Bare Metal: [README](https://github.com/futureretrofusion/EMU68AGA-Bare-Metal) y
  [LICENSE-FRF.md](https://github.com/futureretrofusion/EMU68AGA-Bare-Metal/blob/main/LICENSE-FRF.md).
- Bellatrix: [README](https://github.com/JJDSNT/bellatrix) (no hay `LICENSE`).
- vAmiga: [LICENSE](https://github.com/dirkwhoffmann/vAmiga/blob/master/LICENSE); Moira:
  [LICENSE](https://github.com/dirkwhoffmann/Moira/blob/master/LICENSE); Emu68:
  [LICENSE](https://github.com/michalsc/Emu68/blob/master/LICENSE).
- Dolphin, "fastmem" e "idle skipping": conocido, no consultado en esta vuelta.
- Amiga Hardware Reference Manual: las cifras de PAL (líneas, ciclos de color, reloj E) son de memoria.

Lo que no se comprobó: no hay código, así que nada de esto corre. No se midió el coste de un fallo ni de una llamada
al trampolín. Las cifras de PAL no se han cotejado con el manual. No leí el código de vAmiga, ni las cabeceras de sus
ficheros, ni si emula AGA. No vi cómo sacar de FS-UAE la imagen de los chips ni el sonido sin ventana. Los tamaños y
la cobertura de cada nivel son estimaciones. No estudié cómo trata Emu68 el código que se modifica a sí mismo.
