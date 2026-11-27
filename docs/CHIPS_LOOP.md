# Chips del Amiga, fase 1: plan y bitácora

Encargo del usuario (2026-11-27), por medio de `jefe-dash`: empezar la fase 1 de S18 (`docs/diseño/S18-chips.md`).
Hay que capturar los accesos a `$DFF000` y a los CIA, con un reloj propio de la caja, sobre `caja/motor.h`, para que
valga con Emu68 en ARM y con RelevoJIT en x86. Lo lleva la sesión `dashos-x86`, en la rama `chips` (`.worktrees/chips`,
sale de `origin/main` 4d7cea5b).

Decidido por el usuario el mismo día: el Workbench clásico no se instala (para eso están DashOS, Hangar y Paq). Por
eso la ROM no hace falta para lo que importa, que son los juegos y las demos que tocan los chips. Sin Kickstart va la
ROM de AROS (S36), y la ROM propia sale de la caja nativa más los chips (el modo mixto).

## Qué cambia respecto a S18

S18 es del 2026-11-08 y piensa solo en Emu68. Desde entonces:

- **Hay interfaz de motor** (`caja/motor.h`): Emu68 en ARM y RelevoJIT en x86. Los chips van del lado de la caja,
  sin saber qué motor tienen delante. Cada motor captura los accesos a su manera y llama a los chips.
- **RelevoJIT ya captura sin fallos**: sus páginas `RELEVO_PAGE_HOST` (64 KB) llaman a `host.read` y `host.write`,
  y `relevo_cycles` da la hora en ciclos de 68000. En x86 no hace falta tocar RelevoJIT ni el kernel.
- **En ARM, Emu68 necesita del kernel la entrega del fallo** al propio proceso (S18, sección 2, "Lo que pide al
  kernel", punto 1). Hoy un fallo de página en EL0 mata el proceso (`kernel/src/base/excepciones.rs`). Después: la
  decodificación de la instrucción ARM y la reescritura con un trampolín.
- **La caja de ARM es little-endian** (U7, PR #14). Los chips reciben los valores ya en el orden del 68k, como los
  da `host.read`/`host.write` en RelevoJIT.

## Reglas de cada vuelta

1. **Cuota.** Manda `python3 ~/.claude/scripts/cuota.py`: VERDE adelante, AMBAR nada nuevo y esperas al doble,
   ROJO parar el bucle y los agentes.
2. **Un agente a la vez.** La sesión coordina, revisa y prueba; el agente implementa con papel de implementador.
3. **Sala limpia** (`AGENTS.md`, regla 1). Fuentes: el Amiga Hardware Reference Manual, la hoja de datos del 8520 y
   el núcleo de vAmiga (MPL-2.0; se puede leer y tomar trozos con su aviso, tras revisar las cabeceras). UAE, WinUAE
   y Amiberry no se leen. Nada de la ROM en git.
4. **`unsafe` solo en `kernel/src/base/`.** Los chips van en Rust sin `unsafe`, con pruebas en el PC (`cargo test`).
5. **Cada vuelta**, la sesión comprueba que siguen pasando `make -C kernel prueba` (ARM) y `caja-x86`. Si algo
   está roto y no se arregla, se revierte.
6. **Apuntar.** `[x]` en la tarea y una entrada en la bitácora: qué se hizo, qué se comprobó y qué no.
7. **Hacia fuera.** Sin push ni PR sin el sí del usuario; unir en `main`, con su sí a través de `jefe-dash`. Al
   cerrar cada fase, aviso a `jefe-dash`.
8. **Emulador compartido.** FS-UAE, con el cerrojo `/run/user/1000/dash-emu-red.lock`; se matan solo los PIDs
   propios.
9. **Otras sesiones.** `dashos-aarch64` toca `caja/caja.c`, `caja/programas.S` y `caja/Makefile` en su rama: antes
   de tocar esos ficheros, avisarla.

## Tareas

- [x] K1 **Los chips, en Rust y sin motor.** Un crate nuevo `caja/chips` (`no_std`, sin `unsafe`), con pruebas en el
  PC:
  - **Reloj.** La hora en ciclos de 68000 (PAL, 7.093.790 Hz). El reloj E de los CIA es la hora / 10. Una línea
    son 227,5 ciclos de color (454 de CPU), y un cuadro, 312 líneas (313 en los largos).
  - **Bus.** Qué es cada dirección: CIA-A en `$BFE001` (bytes impares), CIA-B en `$BFD000` (bytes pares), con el
    registro en los bits 8-11. Los chips, en `$DFF000`-`$DFF1FF`. Las demás, a la función de "nadie" (lee 0 y se
    apunta).
  - **Los dos CIA (8520).** Puertos y direcciones (PRA, PRB, DDRA, DDRB). Temporizadores A y B: una vez o
    continuos, carga forzada, cuenta de pulsos del A en el B. Contador TOD: el CIA-A cuenta cuadros y el CIA-B
    líneas, con su alarma y su lectura congelada. ICR, con su máscara y la lectura que borra. CRA y CRB. Todos
    calculados con pereza a partir de la hora.
  - **Agnus mínimo.** `VPOSR`, `VHPOSR` (y `VPOSW`/`VHPOSW`) sacados de la hora. `INTENA`, `INTENAR`, `INTREQ` e
    `INTREQR` con su bit SET/CLR. El nivel de interrupción que pide (`IPL`). `DMACON` y `DMACONR` guardados.
  - **Cola de sucesos.** La hora del siguiente: fin de línea, fin de cuadro (`VERTB`) y desborde de un temporizador.
    Lo que pasa en cada uno sube a `INTREQ`.
- [x] K2 **La interfaz.** En `motor.h`, lo que la caja da al motor: `caja_chip_leer(dir, tam, hora)` y
  `caja_chip_escribir(dir, valor, tam, hora)`. Y lo que el motor da: `motor_ciclos()`, para saber la hora fuera de un
  acceso. Las funciones en C de `pegamento/` llaman a `caja/chips`. Los chips, apagados de serie: se encienden por
  programa (en la cabecera, o una llamada de la caja) o en las pruebas. Sin ellos, la caja sigue como hoy: un acceso
  ahí es un fallo (el Enforcer).
- [x] K3 **RelevoJIT (x86).** Con los chips encendidos, `motor_relevo.c` proyecta `$BF0000`-`$BFFFFF` y
  `$DF0000`-`$DFFFFF` como `RELEVO_PAGE_HOST`, y sus `read`/`write` llaman a K2 con la hora de `relevo_cycles`.
  Una prueba 68k nueva, `caja/pruebas68k/chips.c`: lee `VPOSR` hasta que cambia de línea, pone en marcha el
  temporizador A del CIA-A una vez, espera a su bit de ICR y lee el TOD. Escribe los sucesos en orden, con
  "bien" o "MAL" en cada uno. Va en `caja-x86`. Se miran el intérprete y el JIT, porque el JIT solo estima los
  ciclos.
- [x] K4 **Oráculo.** `chips.exe`, el mismo binario, en FS-UAE con el Kickstart del usuario: las mismas líneas.
  Lo que dependa de tiempos se escribe como orden o como un margen (un temporizador de 709 tics da de 14 a 17
  líneas), no como número exacto. Con el sistema parado mientras mide (`Disable`), y usando temporizadores que el
  Kickstart no tenga en uso.
- [x] K5 **El kernel entrega el fallo al proceso** (ARM primero, x86 después). Una llamada nueva para registrar un
  manejador y otra para reanudar desde un marco (S18, sección 2, "Lo que pide al kernel"). Un fallo dentro del
  manejador mata el proceso, como hoy. Con su prueba en `pruebas.rs` y su entrada en `docs/referencia/kernel.md`.
- [ ] K6 **Emu68 (ARM).** Con los chips encendidos, la caja deja sin proyectar `$BF0000`-`$BFFFFF` y
  `$DF0000`-`$DFFFFF`. El manejador de K5 decodifica el `ldr`/`str` de A64 (tamaño, registro, dirección y
  actualización de la base), llama a K2 y sigue tras la instrucción. La hora sale del contador de instrucciones de
  Emu68 por un coste medio: es aproximada hasta que el traductor sume ciclos. `chips.exe` pasa en `arranque-virt`.
- [ ] K7 **Reescritura (ARM).** Tras el primer fallo en un sitio, la instrucción pasa a un `bl` a un trampolín
  (S18, "Los siguientes: la instrucción reescrita"). Se mide el coste por acceso, con fallo y con trampolín, en QEMU
  (solo para comparar; los tiempos de verdad, en la Pi).
- [ ] K8 **Contador hacia atrás e interrupciones.** El motor vuelve a los chips cuando llega la hora del siguiente
  suceso, y los chips ponen el `IPL`. En la caja nativa, los niveles 6 y 7 ya los usa la caja: aquí solo se dejan
  preparados para la máquina entera (fase 2), sin entregarlos todavía.

Lo que no es de la fase 1: el Copper, el Blitter, Denise, Paula, el disco, la máquina entera y AGA (fases 2 a 7
de S18).

### Fase 2 (adelantada)

- [x] B1 **El Blitter.** De OCS, en `caja/chips/src/blitter.rs`, cableado al bus de `Chips`. Copia (las cuatro fuentes,
  minterms, desplazamientos, máscaras, módulos, descendente), relleno (inclusivo, exclusivo, `FCI`) y líneas (octantes,
  `SING`, textura, acumulador en `BLTAPT`), todo de una vez al escribir `BLTSIZE`; `BZERO`, y `BBUSY` hasta una hora
  estimada en la que sube `BLIT`. La chip RAM la da la caja (`src/memoria.rs`, `Chips::escribir_con` y
  `Chips::avanzar_con`). Oráculo: `caja/pruebas68k/blitter.c` con los casos de `blitter_casos.h`.
- [x] B3 **El audio de Paula.** De OCS, en `caja/chips/src/paula.rs`, cableado al bus de `Chips`. Los cuatro canales
  (`AUDxLC`, `AUDxLEN`, `AUDxPER`, `AUDxVOL`, `AUDxDAT`) con la máquina de estados del manual, el DMA de audio
  (`AUDxEN` y `DMAEN`, una palabra por línea en el ciclo de cada canal), el modo manual, `AUD0`-`AUD3` en `INTREQ` como
  sucesos de `siguiente_suceso`, `ADKCON`/`ADKCONR` con la modulación de volumen y de periodo, y la salida en muestras
  estéreo i16 a una frecuencia dada (`Chips::sonido`, `Chips::muestras`), con un filtro de paso bajo que se puede
  apagar. Oráculo: `caja/pruebas68k/paula.c`.

## Bloqueos

(ninguno)

## Bitácora

### 2026-11-27

- Plan escrito. Rama `chips` desde `origin/main` 4d7cea5b (lleva las PR #13 y #14).

- K1 hecho: `caja/chips` (crate `dashos-caja-chips`, `no_std`, `forbid(unsafe_code)`, sin dependencias, fuera de
  la compilación de la caja hasta K2). `Chips` con `leer`/`escribir` (1, 2 o 4 bytes, orden del 68k),
  `avanzar`, `siguiente_suceso` e `ipl`; los dos CIA en `src/cia.rs`. Fuentes: el Hardware Reference Manual y la
  hoja de datos del 8520, de memoria; no se leyó vAmiga ni nada de UAE o AROS.
  - Corrección al plan: en PAL todas las líneas tienen 227 ciclos de color (454 de CPU, 64 µs). El "227,5" y la
    alternancia de líneas de 227 y 228 son de NTSC. Cuadro de 313 líneas con `LOF` (de serie) y 312 sin él; `LOF`
    cambia en cada cuadro solo con entrelazado (`BPLCON0` bit 2, lo único que se guarda de `BPLCON0`).
  - Comprobado: `cargo test` (37 pruebas por el bus y 2 internas), `cargo clippy` sin avisos, y que compila sin `std`
    para `x86_64-unknown-none` y `aarch64_be-unknown-none-softfloat` (`-Zbuild-std=core`). No se tocó nada
    compartido, así que no se probó `make -C kernel`.
  - Dudas para el oráculo (K4), apuntadas como "Duda:" en el código: el pulso del TOD del CIA-A y `VERTB` caen al
    principio del cuadro y el del CIA-B al principio de la línea (en el Amiga, VSYNC y HSYNC van algo después); el
    temporizador desborda cada `latch + 1` pulsos y empieza a contar en el pulso siguiente a START (sin los dos
    ciclos de retraso del 6526); la línea /IRQ de los CIA sube `PORTS`/`EXTER` por nivel (vuelve a subir si se borra
    `INTREQ` antes de leer `ICR`); el TOD cuenta desde el reinicio y la alarma solo salta al contar; el pin CNT se
    toma alto y quieto; los espejos de los CIA van a "nadie"; escribir una posición del haz fuera del cuadro se
    queda en el borde; el bit 14 de `INTREQ` se guarda; `BZERO` a 0; las entradas de los puertos, a 1.

- K2 y K3 hechos (papel de implementador; fuentes: `caja/chips` de K1, el manual del hardware y la hoja de datos del
  8520; nada de UAE, WinUAE, FS-UAE, Amiberry ni AROS).
  - `caja/motor.h`: lo que la caja da al motor, `caja_chip_leer(dir, tam, hora)` y `caja_chip_escribir(dir, valor,
    tam, hora)` (orden del 68k); lo que da el motor, `motor_ciclos()` y `motor_chips(encendidos)`, que devuelve 1 si
    quedan como se pide. Las dos primeras, en `pegamento/src/chips.rs`, sobre `caja_chips::Chips` (dependencia nueva
    de `pegamento`), guardado en la `Caja` de `lib.rs`: sin `unsafe` nuevo.
  - Cómo se encienden: los enciende la caja entre programas, para el que los necesita (`chips()` en `caja.c`: primero
    `caja_chips_poner(1, motor_ciclos())`, unos chips nuevos con su hora 0 ahí, y después `motor_chips(1)`). El
    programa no hace nada: así `chips.exe` es el mismo binario que irá a FS-UAE en K4. Hoy solo para `chips.exe`; más
    adelante, una marca del programa. Al apagarlos, `caja_chips_poner(0, 0)` dice cuántos accesos fueron a "nadie".
  - `motor_chips(1)` dice 0 si la RAM del 68k ya llega a `$BF0000` (exec puede tener bloques ahí), y con los chips
    encendidos `caja_ampliar_ram` no crece hasta ellos. Pendiente: la RAM de la caja crece seguida desde `$100000`
    hasta 3 GB y pasa por encima de `$BF0000` y `$DF0000`; para encender los chips con un programa grande habrá que
    dejar esos huecos fuera de la RAM (o crecerla por encima de `$E00000`).
  - Emu68 (`motor_emu68.c`): `motor_ciclos` = el contador de instrucciones (`CTX_INSN_COUNT`, v20) por 8;
    `motor_chips(1)` da 0 (K6). En ARM la caja no lanza `chips.exe` y dice "chips: sin chips en este motor".
  - RelevoJIT (`motor_relevo.c`, rev f74486e: tiene `relevo_host.read/write`, `RELEVO_PAGE_HOST`, `relevo_map` y
    `relevo_cycles`): con los chips encendidos, las páginas de 64 KB `$BF0000` y `$DF0000` pasan a
    `RELEVO_PAGE_HOST`, y vuelven a `RELEVO_PAGE_NONE` al apagarlos, con `relevo_invalidate_all`. El `host.read/write`
    ya servía la primera página (la página 0 de verdad): ahora `anfitrion_leer/escribir` mira la dirección y manda lo
    de los chips a `caja_chip_*` con la hora de `relevo_cycles`.
  - `caja/pruebas68k/chips.c` (`chips.exe` en git): con `Disable()`, la línea del haz cambia a la siguiente; el
    temporizador A del CIA-A una vez con 709 tics sube TA en el ICR entre 14 y 17 líneas, se para solo, y leer el ICR
    lo borra; el TOD del CIA-A cuenta 1 entre dos cuadros y 5 en cinco (leído en la línea 20, tras el VSYNC); INTREQ
    con SET y CLR de `SOFT`. Escribe con el sistema en marcha, después. Va tras `saludo.exe` (prueba 4b de `caja.c`),
    con las líneas en `kernel/pruebas/caja_x86.txt` y en `arranque_virt.txt` (las de x86 con `#x86`, la de ARM entre
    `#[arm` y `#]`). "Duda:" en su cabecera sobre los temporizadores que usa el Kickstart.
  - Comprobado: `cargo test` de `caja/chips`; `make -C kernel caja-x86` con `MOTOR=interprete` y con `MOTOR=jit`
    (BIEN los dos): 16 líneas hasta TA, 4 lecturas del haz por línea, TOD +1 y +5, 0 accesos a nadie, en los dos
    motores (el bucle de espera va por el intérprete aun con el JIT: cada acceso a una página del anfitrión pasa por
    él). `make -C kernel prueba` (ARM). No comprobado: `arranque-virt` (ARM) ni `arranque-x86`, que llevan las líneas
    nuevas de `arranque_virt.txt`; ni FS-UAE (K4).
  - Toca `caja/caja.c`, `caja/programas.S` y `caja/Makefile` (regla 9: avisar a `dashos-aarch64`): la prueba 4b, el
    tope de la RAM con los chips, `prog_chips` y la dependencia de `chips.exe`.

- K5 hecho en ARM (papel de implementador; fuentes: el manual de la arquitectura ARMv8-A para ESR, FAR y SPSR, y el
  código del propio kernel). Rama `chips-fallo`.
  - Llamadas nuevas: **61** `fallo_manejador(entrada, pila, tamaño)` (entrada 0 lo quita) y **62** `reanudar(marco)`.
    En x86 las dos dan `E_NO_HAY`. Documentadas en `docs/referencia/kernel.md`, "Fallos del propio proceso".
  - Por hilo, no por proceso: el estado (entrada, fin de la pila y "dentro") va en el `Hilo` del planificador y solo
    lo toca el propio hilo, sin cerrojo. Los hilos nuevos nacen sin manejador.
  - Se entregan las clases 0x00 (no definida), 0x20 y 0x24 (fallo de instrucción y de datos: traducción, permisos,
    alineación) y 0x22 y 0x26 (PC y SP sin alinear). La FPU atrapada (C28) se trata antes, como hoy; el kernel no
    tiene más fallos propios de EL0 (no hay proyección bajo demanda ni pila que crezca). SError, BRK y los registros
    del sistema prohibidos (0x18) siguen matando.
  - El marco (288 bytes: x0-x30, SP, PC, PSTATE, ESR, FAR) va en lo alto de la pila del manejador, **en el orden de
    bytes del proceso** (la caja es big-endian), escrito y leído con las copias de C31 (permisos de EL0). Al entrar,
    x0 = SP = marco y x30 = 0. `reanudar` toma de PSTATE solo NZCV; rechaza un M distinto de EL0t de AArch64 y D, A o
    I enmascaradas, y un PC fuera de la mitad baja. Un fallo con "dentro" puesto, o un marco que no se puede escribir,
    mata el proceso como antes.
  - `unsafe`: ninguno nuevo fuera de `kernel/src/base/` (la lógica está en `base/excepciones.rs` y
    `base/planificador.rs`; `llamadas.rs` solo despacha).
  - Biblioteca: `dashos::fallos` (`fallo_manejador`, `fallo_quitar`, `reanudar`, `MarcoFallo` con un `assert` de
    288 bytes en compilación; el kernel lleva su `MARCO_FALLO` = 36 × 8).
  - Prueba: `usuario/src/bin/fallos.rs`, lanzado por `prueba_fallos` en `pruebas.rs`, en little-endian y en
    big-endian. Lee y escribe en una dirección sin proyectar (el manejador mira FAR, ESR y la instrucción, pone el
    registro destino o ve el valor escrito, PC + 4 y reanuda), ejecuta `udf #0` (el manejador comprueba que un marco
    con EL1h se rechaza y reanuda con Z puesta) y al final falla dentro del manejador: el kernel ve el proceso muerto
    con `Fin::Fallo`. En x86, `prueba_fallos` de `pruebas_procesos_x86.rs` comprueba `E_NO_HAY`.
  - Comprobado: `make -C kernel prueba` (ARM, BIEN, con las dos líneas K5), `make -C kernel prueba-x86` (BIEN, con la
    línea K5 de `E_NO_HAY`) y `make -C kernel caja-x86` (BIEN).
  - No comprobado: la Pi de verdad; los fallos de instrucción y de alineación (solo se prueban datos y la instrucción
    no definida); varios hilos con manejador a la vez.

- K4 (la sesión): `chips.exe` en FS-UAE con el Kickstart 3.1 del A1200 (`emu_run.py --machine a1200`). La primera
  vez, el TOD contó +3 en "cinco cuadros": en el Amiga, el acceso largo a `VPOSR`/`VHPOSR` son dos lecturas, y al pasar
  de la línea 255 a la 256 entre ellas sale la línea 0, un cuadro falso. La imitación lee las dos a la misma hora y no
  lo ve. Arreglado en la prueba (lee hasta que dos lecturas coinciden). Ahora, las seis "bien" en FS-UAE y en
  `caja-x86`. Diferencia que no es fallo: 9 lecturas del haz por línea en el A1200 de FS-UAE y 4 en la caja.
  Las dudas de K1 siguen sin mirar una a una: las pruebas no las distinguen.
- `arranque-virt` (ARM): BIEN con K2 y K3 (la caja dice "sin chips en este motor"). La primera vez falló el guion del
  selector de guardar (no escribió el nombre); repetido, bien. Intermitente, no de los chips.
- `arranque-x86`: no llegaba a arrancar desde la unión de `main` (el disco de prueba pedía `blanker` y `nocapslock`
  sin compilarlos para x86); arreglado (04e2f684). Ahora falla en "cuentas-2" del guardián: en x86 el guardián da el
  Shell por acabado antes de que acabe (línea 3187 frente al fin del Shell en la 3827), y Berta aún no existe. Viene
  de `main` (PR #11) y no de los chips: avisado a `jefe-dash`.

- B1 hecho (papel de implementador; rama `chips-blitter`, sale de `chips`): el Blitter de OCS, adelantado de la fase 2.
  - Fuentes: el Hardware Reference Manual (capítulo 6), de memoria, y vAmiga (`Core/Components/Agnus/Blitter`,
    MPL-2.0, cabeceras revisadas): de `FastBlitter.cpp`, `Blitter.cpp` y `BlitterRegs.cpp` sale el orden de cada
    palabra y de cada punto, el desplazador, el relleno y lo que hace cada escritura de registro; de los
    microprogramas de `SlowBlitter.cpp`, los ciclos por palabra según `USEx`. Reescrito en Rust, con el aviso MPL y el
    origen en la cabecera de `blitter.rs`. Nada de UAE, WinUAE, FS-UAE (su código), Amiberry ni AROS.
  - `src/memoria.rs`: el trait `MemoriaChip` y `SinMemoria`, tal cual los pide el encargo (la sesión del Copper hace
    el mismo fichero). `Chips::escribir_con` y `Chips::avanzar_con` reciben la chip RAM; `escribir` y `avanzar` son
    esas con `SinMemoria`, así que `pegamento/` no cambia (hoy sus blits leen 0 y no escriben). Punteros de 512 KB
    (`MASCARA_CHIP`, bits 18-1).
  - Cómo es: el blit entero al escribir `BLTSIZE`. `BBUSY` sigue a 1 hasta (2 + palabras × ciclos por palabra) ciclos
    de bus, o 4 por punto en líneas, a 2 ciclos de CPU cada uno; entonces baja y sube `BLIT` en `INTREQ`, como un
    suceso de `siguiente_suceso`. Sin `DMAEN` y `BLTEN`, el blit espera (con `BBUSY` a 1) y se hace al encenderlos.
    Los registros quedan como los deja el blit (punteros, `ASH`, `BSH`, `SIGN`, la última A y B leídas), y
    `BLTBDAT` pasa por el desplazador al escribirlo. Los registros del Blitter no se leen (van a "nadie").
  - Oráculo: `caja/pruebas68k/blitter.c` (`blitter.exe` en git) hace con `OwnBlitter` y `Disable` 42 blits de
    `blitter_casos.h` (24 de copia y relleno, 18 de líneas: los ocho octantes, horizontal, vertical, 45 grados, un
    punto, XOR, `SING`, textura, encadenados sin volver a escribir punteros, ancho 0, módulo negativo, relleno
    ascendente, `IFE` y `EFE` a la vez, `BLTBDAT` desplazada sin `USEB`) sobre 4096 bytes de chip RAM con datos
    conocidos, y escribe el CRC32 y `BZERO` de cada uno; al final, un blit con `BLTEN` apagado. `tests/blitter.rs`
    lee el mismo `blitter_casos.h`, hace los mismos blits y compara con las cifras de FS-UAE (A1200, Kickstart 3.1,
    `emu_run.py --machine a1200`): **los 42 iguales**, y el de sin DMA también (no escribe nada, `BBUSY` a 1, y al
    encender el DMA da la suma del caso 1). En FS-UAE, `BBUSY` se vio a 1 tras cada `BLTSIZE` y `BLIT` subió en todos.
  - Comprobado: `cargo test` (18 pruebas nuevas en `tests/blitter.rs`, a mano y el oráculo, y 3 internas),
    `cargo clippy --all-targets` sin avisos, compila sin `std` para `x86_64-unknown-none` y
    `aarch64-unknown-none-softfloat` (`-Zbuild-std=core`), y `make -C kernel caja-x86` (BIEN; `chips.exe` sigue igual).
  - No comprobado: los tiempos (la duración es una estimación; FS-UAE corre con la CPU a "max" y no sirve para
    medirlos); el robo de ciclos y `BLTPRI`; ECS (`BLTSIZV`/`BLTSIZH`, `BLTCON0L`, más de 512 KB); el Blitter a medias
    (un programa que lee lo que el Blitter está escribiendo ve ya el resultado); A1200 es AGA en FS-UAE, pero en estos
    modos el Blitter de AGA es el de OCS.
  - Dudas, en la cabecera de `blitter.rs`: qué pasa al escribir `BLTSIZE` con un blit en marcha (aquí el anterior se
    da por acabado y sube su `BLIT`); una línea con `USEB` o con ancho distinto de 2; `BZERO` antes del primer blit.

- B3 hecho (papel de implementador; rama `chips-paula`, sale de `chips-blitter`): el audio de Paula de OCS, adelantado
  de la fase 2.
  - Fuentes: el Hardware Reference Manual (capítulo 5 y su diagrama de estados), de memoria, y vAmiga
    (`Core/Components/Paula/Audio/StateMachine*.cpp` y, de `Core/Components/Agnus`, `Agnus.cpp`, `AgnusEvents.cpp`,
    `AgnusDma.cpp` y `Sequencer/SequencerDas.cpp`; MPL-2.0, cabeceras revisadas): las transiciones y lo que hace cada
    una, el contador de largo, el retraso de un ciclo de color de `AUDx`, y que una petición de DMA pasa a Agnus al
    acabar la línea y se sirve en la siguiente en el ciclo `$0D` + 2 × canal. Reescrito en Rust, con el aviso MPL y
    el origen en la cabecera de `paula.rs`. Nada de UAE, WinUAE, FS-UAE (su código), Amiberry ni AROS.
  - Cómo es: con pereza, de suceso en suceso (fin de periodo, lectura de DMA, subida de `AUDx`) en orden entre los
    cuatro canales. `siguiente_suceso` mira adelante en una copia, sin chip RAM, hasta la primera `AUDx` o el resto
    de sucesos (como mucho un cuadro); si un canal que modula lee una palabra antes, da esa hora. `AUDx` sube siempre;
    la enmascara `INTENA`. `Chips::leer_con` es nuevo: una lectura también pone al día el DMA de audio, y con `leer`
    (sin chip RAM) lee 0.
  - La salida: apagada de serie. `Chips::sonido(frecuencia, filtro)` la enciende; al poner al día los chips se toma una
    muestra en cada instante de salida con lo que suena entonces (canales 0 y 3 a la izquierda, 1 y 2 a la derecha,
    byte × volumen × 2), en un anillo de 2048 (unos 43 ms a 48 kHz) dentro de `Chips` (8 KB más); `Chips::muestras`
    pone al día y saca. Si nadie las saca se pierden las viejas (`muestras_perdidas`). El filtro es un polo a 4500 Hz,
    aproximado al fijo del A500; el del LED no está. No la usa nadie todavía.
  - Lo que se vio: tras cada `AUDx` se escriben `AUDxLC`/`AUDxLEN` del bloque siguiente al que empieza (el que empieza
    ya los tomó). La primera `AUDx` llega en la línea siguiente al encendido. Con `AUDxLEN` 1 el primer bloque dura
    dos palabras. Con un periodo bajo 124, el DMA da una palabra por línea y el bloque dura unas `AUDxLEN` líneas.
    Apagar el DMA no para el canal en seco: acaba la palabra y, con `AUDx` a 0, suena una más en modo manual (el
    oráculo lo vio: los casos seguidos en el mismo canal empezaban con el canal aún sonando; ahora `paula.c` espera a
    que se pare).
  - Oráculo: `caja/pruebas68k/paula.c` (`paula.exe` en git; no va en la caja), con `Disable`, mide en FS-UAE (A1200,
    Kickstart 3.1, `emu_run.py --machine a1200`) las líneas hasta la primera `AUDx` y entre las cuatro siguientes en
    11 medidas (`AUDxLEN` de 1 a 500, `AUDxPER` de 60 a 2000, los cuatro canales, `AUDxLEN` cambiado a mitad, dos
    canales a la vez) y el modo manual (20 palabras, canales 0 y 3): **0 fallos**. `tests/paula.rs` repite lo mismo y
    da las mismas líneas con una de diferencia como mucho (4 de las 11 medidas, iguales en todo).
  - Comprobado: `cargo test` (16 pruebas nuevas en `tests/paula.rs`: la hora exacta de cada `AUDx`, el ciclo de cada
    canal, `AUDxLEN` y `AUDxPER` 0, las direcciones que lee el DMA en varios bloques y al cambiar `AUDxLC`/`AUDxLEN`,
    apagar el DMA, sin `DMAEN`, el modo manual, sin `INTENA`, `ADKCONR`, las muestras con su volumen y su lado, el
    volumen nuevo en la palabra siguiente, el anillo lleno, el filtro, la modulación de volumen y los dos oráculos),
    `cargo clippy --all-targets` sin avisos, compila sin `std` para `x86_64-unknown-none` y
    `aarch64-unknown-none-softfloat` (`-Zbuild-std=core`), y `make -C kernel caja-x86` (BIEN; `chips.exe` bien, con 3
    lecturas del haz por línea en vez de 4: cada acceso pone al día también el audio, aunque esté parado).
  - No comprobado: el sonido de verdad (las muestras solo se miran en las pruebas; FS-UAE corre con
    `uae_sound_output = interrupts` y no se compara su salida); la modulación de periodo y la de los dos a la vez (solo
    se prueba la de volumen, y no con el oráculo); el filtro frente a un A500; el robo de ciclos; un Amiga de verdad.
  - Dudas, en la cabecera de `paula.rs`: el puntero de la primera palabra (la que no suena) al encender el DMA (aquí
    el que había, como vAmiga); una escritura de `AUDxDAT` por la CPU con el DMA en marcha cuenta como palabra; el
    orden de dos sucesos a la misma hora; que un canal que modula no suene (del manual; vAmiga repite su último
    byte). Coste: un canal con un periodo muy corto da un suceso por periodo (con `AUDxPER` 1, uno por ciclo de color).
