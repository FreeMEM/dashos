# S17. La caja PowerPC

Del 2026-11-07. El usuario dijo "adelante" ese día. Es análisis y diseño: no hay código.

## El problema

Después de AmigaOS 3.x, el mundo Amiga pasó al PowerPC. Hay tres familias de programas PPC:

- **AmigaOS 4** (Hyperion): todo el sistema es PPC. Los programas son ELF y llaman a las bibliotecas por
  "interfaces".
- **MorphOS**: sistema PPC propio, con la API de AmigaOS 3.x por dentro (la ABox). Los programas son ELF.
- **WarpOS y PowerUP**: los programas de las tarjetas PPC del Amiga clásico (Blizzard PPC, CyberStorm PPC). Corren
  junto a AmigaOS 3.x de 68k y usan sus bibliotecas.

La pregunta es si DashOS puede ejecutar esos programas como ejecuta los de 68k (S4): sin emular la máquina entera,
traduciendo el código PPC a ARM y llevando las llamadas al sistema a bibliotecas nativas.

Lo que ya está decidido y condiciona la respuesta:

- La caja Amiga es un proceso con la memoria compartida por dentro, big-endian y con punteros de 32 bits (S3, S4).
- Las bibliotecas de la caja son nativas, escritas en Dash, y se llaman desde el JIT (S4, PLAN).
- El JIT es el de Emu68, en C, con licencia MPL-2.0. El código propio de DashOS también es MPL-2.0 (PLAN).
- Sala limpia: se lee documentación pública de AmigaOS, MorphOS y AROS; no se lee código de AROS ni de los sistemas
  cerrados (`docs/DISEÑO_LOOP.md`, regla 7).
- En CPU sin big-endian (los Mac) la caja necesita el modo del JIT que da la vuelta a los bytes (U7).

## Lo que se comprobó

- **Emu68 ya tiene un JIT de PowerPC a AArch64.** Está en `third_party/Emu68/src/PPC_*.cpp`: unas 8.000 líneas de
  C++, del mismo autor que el JIT de 68k y con licencia MPL-2.0. Empezó en octubre de 2025 y el último cambio es de
  marzo de 2026. Se activa con `ppc_enable` y corre en otro núcleo, junto al 68k (`documentation/overlays.md`).
- **Está a medias.** Cuenta: unas 90 instrucciones traducidas y 25 comentadas. Faltan `lmw` y `stmw`, las formas con
  actualización de la FPU (`lfsu`, `stfdu`...), todo el grupo 59 (la FPU de precisión simple) y AltiVec entero.
- **Su modelo es el mismo que el del JIT de 68k.** Los registros PPC viven en registros ARM: GPR0 a GPR13 fijos en
  w13 a w29, el resto en un banco que se reparte; LR y CTR también fijos
  (`docs/internals/PowerPCRegisterMapping.md`). Da por hecho memoria big-endian, como el de 68k.
- **Emu68 hace de tarjeta PPC.** `src/ppc/` es un núcleo que corre dentro del PPC emulado y ofrece la
  `powerpc.library` de WarpOS. Su lista de tareas tiene 85 funciones del lado PPC; hay unas 20 hechas. Tres ficheros
  llevan copyright de Dennis van der Boon con licencia MIT; los demás no llevan cabecera.
- **QEMU 11.1.3 instalado en shiva** tiene las máquinas `amigaone`, `pegasos1`, `pegasos2`, `sam460ex` y `mac99`, y
  las CPU 7447, 7457, 440ep, 460ex y e5500. También está `qemu-ppc`, que ejecuta programas de Linux PPC.
- **La toolchain `powerpc-440fp` de Bootlin** ya está en `~/.local/opt/toolchains/`. Compila para el núcleo 440 con
  FPU, el de los Sam440 y Sam460.

No se comprobó: que el JIT PPC de Emu68 compile fuera de Emu68, ni su velocidad, ni la de QEMU con un PPC en un ARM.

## 1. Qué hay que traducir

### La CPU

Todos los sistemas PPC del Amiga usan PowerPC de 32 bits en big-endian. Lo común es el juego básico de
instrucciones: enteros, saltos, registro de condición y FPU de doble precisión. Encima, cada máquina añade algo:

| Máquina | CPU | Extra | Sistemas |
|---|---|---|---|
| Blizzard PPC, CyberStorm PPC | 603e, 604e | Nada | WarpOS, PowerUP, OS4 clásico |
| AmigaOne XE, Pegasos II, Mac G4 | G3 (750), G4 (74xx) | AltiVec en los G4 | OS4, MorphOS |
| Sam440, Sam460 | 440EP, 460EX | Instrucciones del 440 (multiplicar y acumular) | OS4, MorphOS en Sam460 |
| X1000 | PA6T-1682M | AltiVec | OS4 |
| X5000 | P5020, P5040 (e5500) | Ninguno: sin AltiVec | OS4, MorphOS desde 3.10 |
| A1222 | P1022 (e500v2) | SPE en lugar de FPU | OS4 |

Consecuencias:

- **Lo básico más la FPU cubre casi todo.** Es lo que generan los compiladores por defecto para OS4 y MorphOS.
- **AltiVec importa en MorphOS** (máquinas G4) y en el X1000. Son unas 160 instrucciones de vectores de 128 bits.
  Casan bien con NEON, pero el orden de los elementos dentro del vector cambia con el orden de bytes. Es trabajo
  aparte.
- **La SPE del A1222 queda fuera.** Usa el mismo código primario que AltiVec y otra FPU. Los programas para A1222 casi
  siempre existen también para PPC normal, porque el propio A1222 emula la FPU clásica por software.
- **La FPU tiene trampas conocidas.** `fmadd` es fusionada en los dos lados, bien. Pero los NaN por defecto, los
  bits de `FPSCR` y las estimaciones (`fres`, `frsqrte`) difieren. Dolphin tuvo que igualarlas para los juegos.

### Cómo llama cada sistema a sus bibliotecas

Es lo que decide cuánto se reutiliza de la caja 68k.

**MorphOS.** Los programas PPC usan la API de AmigaOS 3.x con la convención del 68k. Dejan los argumentos en los
registros 68k virtuales de una estructura, el `EmulHandle`, que siempre está en el registro r2 (D0 a D7, A0 a A7,
con A6 en el desplazamiento 56). Después llaman a `EmulCallDirectOS` con el desplazamiento de la función. Las
bibliotecas nativas de MorphOS marcan su tabla de saltos con una estructura `EmulLibEntry`: un código de trampa
(`TRAP_LIB` = `0xFF00` y familia) y el puntero a la función PPC. MorphOS no arranca la emulación 68k si un programa
PPC llama a una biblioteca PPC por esa vía. Es la misma idea que la línea A de S4.

**WarpOS y PowerUP.** El código PPC vive junto a un programa 68k:

- WarpOS usa hunks normales. Los objetos marcan el código PPC con `HUNK_PPC_CODE` (`$4E9`) y sus saltos con
  `HUNK_RELRELOC26` (`$4EC`); los ejecutables mantienen el formato de siempre. El arranque es 68k y entra en el PPC
  con `RunPPC` de `powerpc.library`.
- PowerUP carga objetos ELF con `ppc.library` desde un arranque 68k, o con un parche de `LoadSeg`.
- Desde el PPC, las dos llaman a las bibliotecas 68k del sistema con un cambio de contexto (`Run68K`,
  `CallLibrary`...). En el Amiga eso cuesta mucho, porque cruza de una CPU a otra.

**AmigaOS 4.** Es el más distinto:

- Los ejecutables son ELF PPC de 32 bits con la entrada en `0x01000000`, pero el cargador (`elf.library`) los
  reubica siempre, porque todos los programas comparten un espacio de direcciones. El `strip` debe dejar las
  reubicaciones. La entrada es `_start(argstring, arglen, SysBase)`. Desde 4.1 hay objetos compartidos (`.so`).
- r13 apunta a los datos pequeños. El resto es la convención SysV de PPC: argumentos en r3 a r10.
- Las bibliotecas no se llaman por la tabla de saltos sino por **interfaces**: `GetInterface(base, "main", 1,
  NULL)` devuelve una estructura con punteros a funciones, y cada función recibe la interfaz como primer argumento
  (`Self`). `SetFunction` pasa a ser `SetMethod`.
- Cada biblioteca de OS4 lleva además una tabla de saltos 68k (`CLT_Vector68K`), para que los programas 68k la
  usen a través de Petunia, el JIT de 68k de OS4.
- La mayoría de los programas usan `newlib.library`, la biblioteca de C del sistema, por su propia interfaz.

## 2. Lo que hicieron otros

- **MorphOS** pone 68k y PPC en la misma caja (ABox), con la misma memoria y las mismas bibliotecas. Las bibliotecas y
  los DataTypes se comparten en las dos direcciones. Ejecuta WarpOS y PowerUP con envoltorios de su API. Su JIT de
  68k se llama Trance.
- **AmigaOS 4** también mezcla 68k y PPC en un solo espacio: Petunia (JIT) y un intérprete para los demás. Ejecuta
  WarpOS desde 2005. ReWarp, de Dennis Boon, es otro envoltorio de WarpOS para OS4.
- **SonnetAmiga y PowerPCAmiga** reimplementan WarpOS para tarjetas PPC por PCI, con licencia MIT. Ojo: el propio
  proyecto avisa de que una parte contiene código reensamblado del WarpOS original.
- **Emu68 1.1** (en alfa desde febrero de 2026) prepara la traducción PPC a ARM para las PiStorm, con una
  `powerpc.library` compatible con WarpOS. Todavía no está activa para los usuarios.
- **QEMU** emula las máquinas enteras: AmigaOne XE (desde 8.2), Pegasos II (desde 6.1; corre OS4 desde 8.0) y
  Sam460ex. Con ellas se instala OS4 4.1 o MorphOS. Su traductor (TCG) admite AltiVec y tiene salida para AArch64.
- **WinUAE y FS-UAE** emulan las tarjetas PPC del Amiga clásico con QEMU como módulo aparte.

### Traductores que existen

| Traductor | Qué traduce | A ARM64 | Licencia | Para DashOS |
|---|---|---|---|---|
| JIT PPC de Emu68 | PPC 32 bits, sin AltiVec, a medias | Sí, en big-endian | MPL-2.0 | Encaja: misma licencia, mismo modelo, mismo autor que el JIT de 68k |
| Dolphin (JitArm64) | Gekko/Broadway (750 con "paired singles") | Sí, maduro | GPL-2.0 o posterior | Solo como referencia; no se puede meter en la caja MPL |
| QEMU TCG | PPC entero, con AltiVec | Sí | GPL-2.0 (partes LGPL) | Como programa aparte; como oráculo de pruebas |
| Cemu | Espresso (750 de la Wii U) | En un derivado (TesseraEmu) | MPL-2.0 | Se podría mirar y reutilizar; sin comprobar |
| PearPC, SheepShaver | G3/G4 | No: solo x86 | GPL-2.0 | No sirve |

El JIT de Emu68 es el único que cumple las tres cosas a la vez: licencia compatible, salida en AArch64 y registros PPC
en registros ARM como el de 68k. Dolphin es el mejor traductor de un 750 que existe, pero GPL: meterlo obligaría a
poner la caja entera bajo GPL. Leer su código como referencia no choca con la sala limpia (no es código de AROS ni de
un sistema cerrado), pero copiar sí choca con la licencia.

## 3. Cómo encaja con la caja 68k

### Opción A: la misma caja, con dos motores

La caja Amiga de S4 lleva dos JIT: el de 68k y el de PPC. Las tareas 68k y las PPC conviven en la misma memoria y
usan las mismas bibliotecas nativas. Es lo que hacen MorphOS y OS4.

- Gana: una sola implementación de exec, dos, intuition, graphics y lo demás. Los programas que mezclan 68k y PPC
  (todos los de WarpOS y PowerUP) funcionan sin puentes. MUI 68k y una aplicación PPC comparten objetos.
- Cuesta: el binario de la caja crece. Emu68 ya compila los dos JIT juntos, así que no es nuevo.

### Opción B: una caja PPC aparte

Un proceso distinto para lo PPC, con sus propias bibliotecas.

- Gana: aislamiento. Un fallo de un programa PPC no tira el Workbench.
- Cuesta: duplicar bibliotecas, y un puente para cada cosa que se compartía (puertos, ventanas, MUI). WarpOS y PowerUP
  no funcionan sin la parte 68k en la misma memoria.

Ya hay cajas aparte para programas que fallan (S4, punto 5). Con la opción A, un programa PPC dudoso va a una caja
aparte igual que uno 68k, sin diseño nuevo.

### El testigo y los núcleos

Una tarea PPC es un hilo de la caja, como una 68k. Corre solo cuando tiene el testigo (S4, punto 2). Eso cambia una
cosa de WarpOS: en el Amiga, el 68k y el PPC corren a la vez en dos CPU. En DashOS se turnan. Los programas WarpOS se
sincronizan con señales y mensajes, así que turnarse vale; MorphOS hace lo mismo en una sola CPU. Lo que no vale es
el modelo de Emu68 de un núcleo dedicado al PPC: aquí el PPC es un motor más dentro de la caja.

Cambiar de motor es cambiar de hilo: cada hilo guarda sus registros ARM, y los dos JIT usan los mismos (x13 a x29).
La caché de traducciones es una por motor.

### Llamadas entre PPC, 68k y lo nativo

- **De PPC a nativo.** Una instrucción reservada de PPC seguida del número de la función, igual que la línea A en el
  68k. El JIT PPC la reconoce y emite una llamada directa. Va en tres sitios:
  - En la tabla de saltos de cada biblioteca, como `EmulLibEntry` de MorphOS, para los programas MorphOS que llaman con
    `EmulCallDirectOS`.
  - En las entradas de `powerpc.library` y `ppc.library`, para WarpOS y PowerUP. `Run68K` y `CallLibrary` a una
    biblioteca nativa no cambian de motor: llaman directo.
  - En los punteros de las interfaces de OS4.
- **Argumentos.** MorphOS deja los argumentos en el `EmulHandle`: la función nativa los lee de ahí, con la misma
  firma que desde el 68k. OS4 los pasa en r3 a r10, que el JIT tiene en registros ARM; un puente generado los pone en
  x0 a x7. Los puentes salen de los `.sfd` (68k) y de las descripciones de interfaz del SDK de OS4.
- **De nativo a PPC.** `llamar_ppc(dirección, registros)`, como `llamar_68k` de S4, para los ganchos de MUI e
  Intuition y las funciones de comparación.
- **De PPC a 68k.** Una biblioteca o un gancho 68k llamado desde PPC entra en el JIT de 68k. Es `EmulCall68k` en
  MorphOS y `Run68K` en WarpOS.
- **Parches.** `SetFunction` sobre la tabla de saltos lo ven los dos motores. En OS4, `SetMethod` cambia el puntero de
  la interfaz; las llamadas entre bibliotecas nativas miran si sigue siendo la marca nativa, como en S4.

### Qué bibliotecas hay que escribir

Para MorphOS, WarpOS y PowerUP, casi todo ya lo pide la caja 68k: exec, dos, graphics, layers, intuition, gadtools,
utility. Lo nuevo:

| Pieza | Para | Tamaño aproximado |
|---|---|---|
| `powerpc.library` (lado 68k y lado PPC) | WarpOS | 85 funciones PPC y unas 20 del lado 68k (sin contar con exactitud) |
| `ppc.library` y su cargador de ELF | PowerUP | Del orden de 60 funciones (sin contar con exactitud) |
| `EmulHandle`, trampas y cargador ELF de MorphOS | MorphOS | Poco código, pero toca exec y dos |
| Lo que añade MorphOS sobre 3.x (MUI 4 PPC, TinyGL, CyberGraphX, extensiones de exec y dos) | MorphOS | Depende de cada programa |
| Warp3D v4 y MiniGL | Juegos WarpOS y PowerUP | Ya está en S9 como "después" |

Para OS4, en cambio, hay que escribir casi una segunda API:

| Pieza | Por qué cuesta |
|---|---|
| Interfaces de todas las bibliotecas | Cada biblioteca tiene su interfaz `main` y a veces más; exec, dos y graphics de OS4 tienen muchas más funciones que las de 3.1 |
| exec y dos de OS4 (v50 en adelante) | Funciones nuevas (objetos del sistema, mutex, depuración, la nueva API de ficheros de dos) y estructuras ampliadas |
| `elf.library` | Reubicación, objetos compartidos, copia de los datos por proceso |
| `newlib.library` | La biblioteca de C entera, con su interfaz; casi todos los programas la abren |
| Picasso96API, Warp3D Nova, `application.library`... | Lo que pida cada programa; Warp3D Nova necesita la GPU (S9) |

La caja tendría que mostrar las estructuras de OS4 donde amplían las de 3.1 (`ExecBase`, tareas, procesos). Si
amplían por el final, sirven las dos a la vez; donde no, hay que elegir. Eso no se puede saber sin leer el SDK de OS4
a fondo.

## 4. Qué programas merecen la pena

### Primero, lo que no tiene código fuente

Los programas PPC de código abierto se portan mejor en nativo que en la caja: Odyssey y OWB (WebKit, licencia BSD),
NetSurf, ScummVM, Serious Sam (motor publicado, portado a OS4 en 2025). Con el backend ARM de Dash y relibc (S7, S11)
corren a toda velocidad y sin JIT. La caja PPC es para lo que no se puede recompilar:

- **Juegos WarpOS y PowerUP comerciales** de 1997 a 2002 (versiones PPC de Heretic II, Shogo, Wipeout 2097 y otros). Necesitan Warp3D v4. Es el grupo más fácil, porque la API es pequeña y la de 3.x ya está.
- **Aplicaciones MorphOS sin código publicado.** Hay que comprobarlo programa a programa; Wayfarer (navegador) e
  Iris (correo) son candidatas. Necesitan MUI de MorphOS y extensiones del sistema.
- **Aplicaciones y juegos OS4 cerrados.** Hollywood también tiene versión 68k, WarpOS y MorphOS, así que no obliga
  a OS4. Lo que solo existe para OS4 es lo que justificaría la fase más cara.

No hay una lista pública de qué programas existen solo en PPC. Antes de la fase de OS4 hay que hacer esa lista,
mirando OS4Depot, el catálogo de MorphOS y Aminet.

### El sistema entero: con QEMU, no dentro de la caja

Ejecutar OS4 o MorphOS enteros dentro de la caja no es realista: sus núcleos (ExecSG, Quark) mandan sobre el
hardware PPC (MMU, interrupciones, PCI) y esperan una placa concreta. La forma práctica es la de Amiberry en S4:
**QEMU empaquetado como aplicación**, con su ventana en el escritorio.

- **OS4**: se compra a Hyperion para una máquina. La edición de AmigaOne XE corre en la `amigaone` de QEMU con el
  cargador BBoot; la de Pegasos II, en `pegasos2`. QEMU recomienda la versión 10 o posterior por la NVRAM.
- **MorphOS**: sin registrar funciona entero, pero se ralentiza a los 30 minutos hasta reiniciar. La licencia va
  atada a la máquina para la que se genera. No se comprobó si MorphOS registra una máquina emulada.
- **Velocidad**: QEMU traduce el PPC entero, con la MMU, en un núcleo de la Pi 4. No se midió. Un dato de referencia:
  Dolphin, con un JIT muy afinado, mueve juegos de GameCube (un 750 a 486 MHz) en una Pi 5. La Pi 4 es bastante más
  lenta.
- **Licencia**: QEMU es GPL, pero como programa aparte no toca la licencia de DashOS. DashOS no lleva ningún OS4 ni
  MorphOS; los pone el usuario, como la ROM de Kickstart.

## 5. Sala limpia y licencias

- **El JIT PPC de Emu68** es MPL-2.0 y del autor de Emu68. Se puede usar como el JIT de 68k.
- **`src/ppc/` de Emu68 no se reutiliza sin revisarlo antes.** Tres ficheros vienen de Dennis van der Boon, y su
  proyecto de origen (SonnetAmiga) avisa de que contiene código reensamblado del WarpOS original. Además DashOS no lo
  necesita: su `powerpc.library` es nativa, no un núcleo que corre dentro del PPC emulado.
- **La `powerpc.library` y la `ppc.library` se escriben desde la documentación pública**: las autodocs de WarpOS y
  de PowerUP, como el resto de la API.
- **MorphOS**: su SDK público (`emulinterface.h`, `emulregs.h`) documenta la interfaz. Basta para la parte del
  `EmulHandle`.
- **OS4**: el SDK es gratuito, pero su licencia no se ha revisado. Hace falta para las interfaces, como el NDK 3.2
  para la API de 3.1. Ver "Lo que queda abierto".
- **Dolphin y QEMU** se pueden leer para entender casos difíciles de la FPU, pero no se copia nada a la caja. QEMU
  sí se usa tal cual, como programa aparte: de oráculo en las pruebas y de máquina entera para el usuario.
- **El oráculo de PPC.** Para las instrucciones: el mismo programa PPC en `qemu-ppc` y en el JIT, con la misma
  salida, como `jit-host/` hace con el 68k. Para la API: el mismo programa en OS4 o MorphOS dentro de QEMU y en la
  caja, mirando solo su salida, como el oráculo de FS-UAE (PLAN). Ninguno lleva desensamblar el sistema.

## 6. Riesgos

| Riesgo | Dónde se ve | Qué se haría |
|---|---|---|
| El JIT PPC de Emu68 cambia mucho o se queda parado | F1 | Tomarlo en un momento dado y seguir en DashOS; mandar los arreglos a Emu68 |
| La FPU no da los mismos resultados que un PPC real | F1, con el oráculo | Casos especiales, como Dolphin; aceptar diferencias en el último bit donde no importen |
| Dos motores en una caja se pisan (registros, cachés) | F1, F2 | Una caché por motor; cambio de motor solo al cambiar de hilo |
| WarpOS espera de verdad dos CPU a la vez | F2 | Programas que esperan en bucle a la otra CPU: ceder el testigo en esas esperas |
| Los juegos dependen de Warp3D y la GPU no está | F2 | Warp3D por software sobre el compositor al principio, como MiniGL por CPU |
| OS4 pide demasiada API | F4 | Hacer la lista de programas antes; si no compensa, OS4 solo con QEMU |
| CPU sin big-endian (Mac) | Todas | El mismo trabajo de U7, también en el JIT PPC |

## 7. Fases y tamaño

Todo esto va después de que la caja 68k funcione con ventanas (H5). No entra en DashOS 0.1 (S15).

| Fase | Qué | Se enseña | Tamaño |
|---|---|---|---|
| F1 | JIT PPC en `jit-host/`: completar enteros y FPU, la llamada nativa por instrucción reservada, pruebas contra `qemu-ppc` | Un programa PPC compilado con `powerpc-440fp` da la misma salida en `qemu-ppc` y en el JIT | 3.000 a 5.000 líneas sobre las 8.000 de Emu68 |
| F2 | WarpOS y PowerUP en la caja: los dos motores, `powerpc.library`, `ppc.library`, cargador ELF de PowerUP | Una demo WarpOS que abre su ventana | 6.000 a 9.000 líneas, más Warp3D para los juegos |
| F3 | MorphOS: `EmulHandle`, trampas, cargador ELF, y las extensiones que pidan los primeros programas | Un programa MUI de MorphOS en el escritorio | 3.000 líneas de base, más lo que pida cada programa |
| F4 | AltiVec | Un programa con AltiVec da lo mismo que en QEMU | 4.000 a 6.000 líneas |
| F5 | OS4: interfaces, exec y dos de OS4, `elf.library`, `newlib.library` | Un programa OS4 sencillo con ventana | 20.000 a 40.000 líneas; es otra caja de API casi entera |
| Aparte | QEMU empaquetado para OS4 y MorphOS enteros | OS4 arrancando en una ventana | Poco código; empaquetado y ajustes |

Con el ritmo de la caja 68k como referencia, F1 a F3 son del orden de meses de una línea de trabajo, y F5 de un año.
Son estimaciones sin medir: no se ha escrito nada de esto.

## Recomendación

1. **Una sola caja Amiga con dos motores**, 68k y PPC, como MorphOS. Las bibliotecas nativas sirven a los dos. Los
   programas dudosos van a cajas aparte, como en S4.
2. **El JIT PPC de Emu68 como base**, por licencia y por modelo. Los arreglos se mandan a Emu68. Dolphin y QEMU solo
   como referencia y como oráculo, nunca dentro de la caja.
3. **Orden: WarpOS y PowerUP primero, después MorphOS, AltiVec y, al final y solo si la lista de programas lo
   justifica, OS4.** Los tres primeros usan la API de 3.x que la caja 68k ya tiene; OS4 pide una API nueva.
4. **El sistema entero de OS4 o MorphOS, con QEMU empaquetado**, igual que Amiberry para lo que toca el chipset. Es
   barato y sirve desde el principio a quien tenga licencia.
5. **Lo PPC de código abierto se porta en nativo**, no se emula.
6. **Empieza después de H5.** Antes no hay caja en la que meter el motor. F1 sí se puede hacer antes, en `jit-host/`,
   porque no depende del kernel.

## Lo que queda abierto

1. ¿Una sola caja con 68k y PPC, o una caja PPC aparte? Recomendado: una sola.
2. ¿Este orden (WarpOS y PowerUP, MorphOS, AltiVec, OS4 al final)? Recomendado: sí.
3. ¿Se hace OS4 en la caja, o OS4 solo con QEMU entero? Recomendado: decidirlo tras la lista de programas solo-OS4.
4. ¿Se puede usar el SDK de OS4 para sacar las interfaces? Hay que leer su licencia antes de F5.
5. ¿QEMU empaquetado como aplicación, como Amiberry? Recomendado: sí.
6. ¿F1 en `jit-host/` ya, en paralelo, o esperar a H5? Recomendado: puede ir ya, si hay manos libres.

## Cómo encaja

- Con S4: la caja Amiga gana un segundo motor; la línea A tiene su gemela en PPC; `llamar_ppc` es la gemela de
  `llamar_68k`; las cajas aparte sirven también para PPC.
- Con S3: misma memoria de 4 GB, big-endian, Enforcer para los dos motores.
- Con S9: Warp3D v4 y MiniGL ya estaban en "después"; los juegos PPC los necesitan. Warp3D Nova de OS4 necesita la
  GPU.
- Con S11: los puentes se generan como los de 68k; la API nativa no cambia.
- Con S15: va después de DashOS 0.1, junto a la GPU y la Pi 5. F1 puede ir antes en `jit-host/`.
- Con U7: el modo sin big-endian hace falta también en el JIT PPC.
- Con la sala limpia: nada de código de OS4, MorphOS ni AROS; `src/ppc/` de Emu68 se revisa antes de usarlo.

## Fuentes

- JIT PPC de Emu68: `third_party/Emu68/src/PPC_*.cpp`, `include/PPC.h`, `docs/internals/PowerPCRegisterMapping.md`,
  `documentation/overlays.md` y `src/ppc/libfunc/todo.md`, en el clon del repositorio.
- Emu68 1.1 en alfa: [Generation Amiga](https://www.generationamiga.com/2026/02/11/emu68-alpha-update-marks-major-step-forward-in-emulator-performance/)
  y [Prism News](https://www.prismnews.com/hobbies/retro-game-emulation/emu68-v110-alpha1-adds-4-way-jit-cache-ppc-to-arm-groundwork).
- Interfaz de emulación de MorphOS: [emulinterface.h del SDK](https://morphos-team.net/sdk/includes/emul/emulinterface.html),
  [el `EmulHandle` en r2 (wiki de Lazarus)](https://wiki.lazarus.freepascal.org/MorphOS),
  [hilo sobre bibliotecas PPC con ABI 68k](https://forums.hollywood-mal.com/viewtopic.php?p=12533).
- MorphOS en general, máquinas y versión 3.20: [Wikipedia](https://en.wikipedia.org/wiki/MorphOS); licencia y modo
  sin registrar: [FAQ de MorphOS](https://www.morphos-team.net/faq).
- AmigaOS 4: [Wikipedia](https://en.wikipedia.org/wiki/AmigaOS_4),
  [Exec de OS4 e interfaces](https://wiki.amigaos.net/wiki/Programming_AmigaOS_4:_Exec_-_The_Kernel),
  [crear una biblioteca de OS4](https://wiki.amigaos.net/wiki/How_to_create_an_AmigaOS_4_library),
  [bibliotecas y dispositivos](https://wiki.amigaos.net/wiki/Libraries_and_Devices),
  [Petunia y la emulación 68k](https://amigaos.net/content/127/extensive-68k-emulation-capabilities).
- ELF de OS4: [suplemento de la ABI ppc-amigaos (AmigaLabs)](https://github.com/AmigaLabs/binutils-gdb/wiki),
  [elf.library 54.1](https://amiga-news.de/en/news/AN-2024-01-00097-EN.html).
- WarpOS y PowerUP: [Wikipedia](https://en.wikipedia.org/wiki/WarpOS),
  [hunks ampliados de Haage & Partner](https://discmaster.textfiles.com/file/1049/Amiga%20Format%20CD38%20(1999-03-15)(Future%20Publishing)(GB)(Track%201%20of%203)[!][issue%201999-04].iso/-websites-/haage&partner/sc_tec_e.htm),
  [ppclibemu](https://generic.aminet.net/util/libs/ppclibemu.readme),
  [SonnetAmiga](https://github.com/sakura-it/sonnetamiga), [PowerPCAmiga](https://github.com/Sakura-IT/PowerPCAmiga),
  [ReWarp](https://aminet.net/package/util/libs/ReWarp).
- Máquinas y CPU: [AmigaOne X1000](https://en.wikipedia.org/wiki/AmigaOne_X1000),
  [AmigaOne X5000](https://amigaos.net/hardware/133/amigaone-x5000),
  [el A1222 y la SPE](https://keasigmadelta.com/blog/taborizing-drivers-and-libraries-a-k-a-optimizing-for-the-e500-spe/).
- QEMU: [máquinas AmigaNG](https://qemu.readthedocs.io/en/master/system/ppc/amigang.html),
  [OS4 en QEMU, páginas de BALATON Zoltan](https://zero.eik.bme.hu/~balaton/qemu/amiga/).
- Dolphin: [repositorio y licencia](https://github.com/dolphin-emu/dolphin),
  [Dolphin en la Pi 5](https://www.tomshardware.com/news/raspberry-pi-5-emulates-nintendo-gamecube-and-wii).
- Cemu y su licencia MPL: [PR del recompilador](https://www.github.com/cemu-project/Cemu/pull/641),
  [derivado con JIT de ARM64](https://github.com/PowerBeef/Cemu-MacOS) (sin comprobar).
- Programas: [Origyn y Odyssey](https://en.wikipedia.org/wiki/Origyn_Web_Browser),
  [Serious Sam en OS4](https://www.generationamiga.com/2026/02/22/serious-sam-reborn-bringing-arena-fps-action-to-amigaos-4-and-amigaone/),
  [juegos de OS4](https://www.generationamiga.com/2026/04/02/50-amazing-games-you-can-play-on-amigaos-4-and-amigaone-computers/).

Lo que no se comprobó: nada se ha compilado ni ejecutado del JIT PPC; no se midió la velocidad de QEMU con un PPC en
ARM; no se leyó la licencia del SDK de OS4; no se comprobó si MorphOS deja registrar una máquina emulada; el derivado
de Cemu con ARM64 se cita por su README, sin mirar el código.
