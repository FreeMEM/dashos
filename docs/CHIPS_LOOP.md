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
- [x] K6 **Emu68 (ARM).** Con los chips encendidos, la caja deja sin proyectar `$BF0000`-`$BFFFFF` y
  `$DF0000`-`$DFFFFF`. El manejador de K5 decodifica el `ldr`/`str` de A64 (tamaño, registro, dirección y
  actualización de la base), llama a K2 y sigue tras la instrucción. La hora sale del contador de instrucciones de
  Emu68 por un coste medio: es aproximada hasta que el traductor sume ciclos. `chips.exe` pasa en `arranque-virt`.
- [x] K7 **Reescritura (ARM).** Tras el primer fallo en un sitio, la instrucción pasa a un `bl` a un trampolín
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
- [x] B2 **El Copper.** `caja/chips/src/copper.rs`, cableado a `Chips`: `COP1LC`, `COP2LC`, `COPJMP1`, `COPJMP2`,
  `COPCON` (`CDANG`) y `COPINS`; `COPEN` y `DMAEN` en `DMACON`. `MOVE` (con los registros prohibidos), `WAIT` y `SKIP`
  con sus máscaras y `BFD`, contra la posición del haz y con pereza. La chip RAM la da la caja (`src/memoria.rs`,
  `MemoriaChip`). Los registros de color y `BPLCON0`/`1`/`2` se guardan para Denise. Oráculo:
  `caja/pruebas68k/copper.c`.
- [x] B3 **El audio de Paula.** De OCS, en `caja/chips/src/paula.rs`, cableado al bus de `Chips`. Los cuatro canales
  (`AUDxLC`, `AUDxLEN`, `AUDxPER`, `AUDxVOL`, `AUDxDAT`) con la máquina de estados del manual, el DMA de audio
  (`AUDxEN` y `DMAEN`, una palabra por línea en el ciclo de cada canal), el modo manual, `AUD0`-`AUD3` en `INTREQ` como
  sucesos de `siguiente_suceso`, `ADKCON`/`ADKCONR` con la modulación de volumen y de periodo, y la salida en muestras
  estéreo i16 a una frecuencia dada (`Chips::sonido`, `Chips::muestras`), con un filtro de paso bajo que se puede
  apagar. Oráculo: `caja/pruebas68k/paula.c`.
- [x] B4 **Los planos y Denise.** `caja/chips/src/denise.rs`: el DMA de planos de Agnus (`BPL1PT`-`BPL6PT`,
  `BPL1MOD`/`BPL2MOD`, `DDFSTRT`/`DDFSTOP`, la ventana vertical, `BPLEN`; los ciclos que usa no los tiene el Copper) y
  Denise de OCS (`BPLxDAT`, `BPLCON0`-`2`, `COLOR00`-`31`, la ventana horizontal): baja y alta, 1 a 6 planos, EHB,
  HAM6, campos dobles con `PF2PRI`, desplazamiento fino, borde en `COLOR00`, campos del entrelazado. A nivel línea,
  con los cambios a mitad de línea en su sitio. La imagen (752 x 286, `0x00RRGGBB`) la recibe quien da la chip RAM
  (`MemoriaChip::linea_de_video`, `ConPantalla`). Oráculo: `caja/pruebas68k/denise.c` con `denise_modos.h`, contra
  capturas de FS-UAE.
- [x] B5 **Los CIA completos.** Las dudas de K1 resueltas con medidas en FS-UAE (`caja/pruebas68k/cia.c`, A1200 y
  A500) y vAmiga; el registro serie en los dos sentidos; el teclado del Amiga en el del CIA-A (`src/teclado.rs`,
  `Chips::tecla`), con su saludo y la resincronía; los puertos (`Chips::ovl`, `led`, `fuego`, `disco_senales`,
  `disco_lineas`) y `Chips::tras_el_arranque`. Pruebas: `caja/chips/tests/cia.rs`.
- [x] B6 **Los sprites y los choques.** `caja/chips/src/sprites.rs`: el DMA de sprites de Agnus (`SPRxPT`, las
  palabras de control y de datos en sus ciclos, `VSTART`/`VSTOP` con sus bits altos, el fin de la lista, varios
  sprites por canal, `SPREN`, los ciclos que se comen los planos con `DDFSTRT` temprano) y Denise (los 8 sprites,
  unidos, `SH0`, armar y desarmar, el modo a mano, prioridades con `BPLCON2` con un campo y con dos, solo dentro de
  la ventana), y los choques (`CLXCON`, `CLXDAT`). Oráculo: `caja/pruebas68k/sprites.c` con `sprites_escenas.h`,
  contra capturas de FS-UAE y su `CLXDAT`.

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
- B2 hecho (papel de implementador; rama `chips-copper`, sale de `chips`). Fuentes: el Hardware Reference Manual, de
  memoria, y el Copper del núcleo de vAmiga (`Core/Components/Agnus/Copper`, cabeceras MPL-2.0 revisadas), del que se
  toman los pasos de cada instrucción, el comparador y los registros prohibidos; el código está escrito de nuevo y lleva
  el aviso de dónde sale. No se leyó UAE, WinUAE, FS-UAE (su código), Amiberry ni AROS.
  - `src/copper.rs`: una máquina de estados por pasos de bus, cada uno a su hora (`Chips::avanzar_con` da cada paso con
    lo demás al día hasta él, así que lo que escribe queda en orden con `VPOSR`, `VERTB` y los CIA). Un `MOVE` son dos
    pasos (4 ciclos de color); un `WAIT`, tres y despierta 2 ciclos antes de la posición; un `SKIP`, cuatro. Solo
    ciclos pares, y nunca el `$E0`. La línea se compara en 8 bits (el `WAIT $FFDF` funciona por esos tiempos: el
    `WAIT` siguiente ya mira en la 256). Un `WAIT` que no llega en el cuadro y un `MOVE` prohibido lo paran hasta el
    cuadro siguiente; al empezar cada uno vuelve a `COP1LC`. Prohibidos (OCS): sin `CDANG`, `$000`-`$07E`; con él,
    `$000`-`$03E`. Un `MOVE` escribe por `escribir_custom`, el mismo camino que el 68k. `blitter_ocupado()` da `false`
    hasta que llegue el Blitter.
  - `src/memoria.rs` (`MemoriaChip`, `SinMemoria`), igual que en la rama del Blitter. `Chips` gana `avanzar_con`,
    `leer_con` y `escribir_con`; `avanzar`, `leer` y `escribir` llaman a esas con `SinMemoria`, y `pegamento/`
    no cambia.
    `MASCARA_CHIP` = 512 KB (OCS). `siguiente_suceso` no pasa del siguiente paso del Copper si está en marcha (en un
    `WAIT`, la hora en que despierta).
  - `COLOR00`-`COLOR31` (12 bits) y `BPLCON0`/`1`/`2` se guardan (`Chips::color`, `Chips::bplcon`). Escribir `COLOR00`
    ya no cuenta como acceso a nadie (cambiada esa línea de `direcciones_de_nadie`).
  - Oráculo: `caja/pruebas68k/copper.c` (`copper.exe`, en `make` de `pruebas68k` y en git). Una lista propia en chip RAM
    avisa con `MOVE` a `INTREQ` (`SOFT` o `COPER`) y el 68k, con `Disable` y `OwnBlitter`, apunta en qué línea ve cada
    bit. En FS-UAE con el Kickstart 3.1 del A1200 (`--machine a1200`): las ocho "bien", líneas 40, 80, 120, 286, 300 y
    40 (`WAIT`, `SKIP` que se cumple y que no, `WAIT $FFDF` y la línea 30, `COPJMP2` desde la lista, vuelta a `COP1LC`),
    sin `CDANG` el `MOVE` a `$044` para el Copper todo el cuadro, y con `CDANG` se hace (línea 40). La misma lista en
    `tests/copper.rs` (`oraculo_copper_c`) da las mismas líneas exactas.
  - Comprobado: `cargo test` (37 + 20 pruebas, más 2 internas), `cargo clippy --all-targets` sin avisos, que compila sin
    `std` para `x86_64-unknown-none`, `aarch64-unknown-none-softfloat` y `aarch64_be-unknown-none-softfloat`
    (`-Zbuild-std=core`), y `make -C kernel caja-x86` (BIEN; en este worktree hizo falta enlazar `third_party/Emu68` y
    `pkg2` del de `chips` y `make -C kernel usuario` antes).
  - No comprobado: la exactitud al ciclo frente al Amiga (el oráculo solo distingue líneas); el robo de ciclos de los
    planos y los demás DMA (aquí el Copper tiene todos los pares); `BFD` con un Blitter de verdad; ECS (con `CDANG`
    puede escribir todo) y AGA. La caja aún no da chip RAM a los chips, así que `copper.exe` no va en `caja-x86`.
  - Dudas: el oráculo es un A1200 (AGA): la diferencia de OCS con `CDANG` (`$000`-`$03E` prohibidos) no se pudo ver
    ahí, sale de vAmiga. Un `SKIP` que se cumple solo salta un `MOVE`; si lo siguiente es un `WAIT`, espera igual (como
    vAmiga; el manual dice "la instrucción siguiente"). Escribir `COPINS` no hace nada. Escribir `COP1LC` o `COP2LC`
    mueve también el contador si el cuadro empezó con el DMA del Copper apagado (como vAmiga). Al encender el DMA, el
    Copper sigue donde estaba, sin esperar a un ciclo par.

- Unión de B1 y B2 en `chips-blitter` (la sesión). Conflictos en `lib.rs` (los dos añadían `escribir_con` y
  `avanzar_con`, y cada uno su módulo `dma`: queda el del Blitter, que tiene todos los bits), en el `Makefile` de
  `pruebas68k` y aquí. Cosido lo que cada uno dejó para el otro: `blitter_ocupado` del Copper es ya `BBUSY`; un
  `MOVE` del Copper a `BLTSIZE` hace el blit con la chip RAM; encender `DMACON` avisa a los dos; y con el Copper
  parado en un `WAIT` con `BFD` a 0, `avanzar_con` para en el fin del blit para que siga a su hora (sin eso saltaba
  hasta el fin del cuadro). Prueba nueva `wait_con_bfd_a_0_espera_al_blitter`. Comprobado: `cargo test` (todas) y
  `cargo clippy --all-targets` sin avisos. `make -C kernel caja-x86` BIEN. No comprobado: `BFD` en el oráculo.
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

- Unión de B3 en `chips-blitter` (la sesión), sobre B1 y B2. Conflictos en `lib.rs`: el paso de la hora del Copper
  (`avanzar_base`) recibe ahora la chip RAM, para que el DMA de audio lea a la hora de cada paso; los comentarios,
  juntados. Comprobado: `cargo test` (todas) y `cargo clippy --all-targets` sin avisos; `make -C kernel caja-x86`
  BIEN. No comprobado: el Copper escribiendo en los registros de audio en el oráculo.

- B4 hecho (papel de implementador; rama `chips-denise`, sale de `chips-blitter`): los planos y Denise de OCS.
  - Fuentes: el Hardware Reference Manual, de memoria, y vAmiga (`Core/Components/Denise` y
    `Core/Components/Agnus/Sequencer`, cabeceras MPL-2.0 revisadas): el orden de los planos en la unidad de lectura y
    la máquina de `DDFSTRT`/`DDFSTOP` de OCS, el biestable vertical, cuándo carga Denise sus registros según `BPLCON1`
    y dónde caen los 16 píxeles, el contador horizontal y el biestable de la ventana (borde hasta el primer
    `BPL1DAT`), y la traducción de campos dobles, EHB y HAM. Reescrito en Rust, con el aviso MPL y el origen en la
    cabecera de `denise.rs`. Nada de UAE, WinUAE, FS-UAE (su código), Amiberry ni AROS.
  - Cómo es: con pereza, ciclo de color a ciclo de color dentro de la línea. `Chips::avanzar_con` pone la imagen al
    día cuadro a cuadro (`pantalla_hasta`) antes de lo demás; antes de cada escritura en un color, `BPLCON0`,
    `BPLCON2` o la ventana, Denise traduce a colores lo que ya se ve con lo de antes (un cambio en el ciclo `h` vale
    desde la posición `2h`). `MemoriaChip` gana tres métodos con valor de serie (`quiere_video`, `linea_de_video`,
    `cuadro_de_video`): sin imagen (`SinMemoria`, `pegamento/`), Denise no pinta y Agnus solo lleva sus punteros, así
    que `pegamento/` no cambia. `Chips::cuadros_pintados()` dice cuántos cuadros hay y si el último era largo. Sin
    sprites: su sitio está marcado en `traducir`.
  - La imagen: 752 x 286, posiciones horizontales `$5C` a `$1D3` (en píxeles de baja, una columna por píxel de alta)
    y líneas 26 a 311: lo mismo que el cuadro entero de FS-UAE (su captura "full").
  - El Copper no usa los ciclos de los planos (`planos_usan_ciclo`): en alta con 4 planos, un `MOVE` pedido a mitad
    de la lectura cae al acabarla; en baja con 6, solo le quedan dos ciclos de cada 8.
  - Oráculo: `caja/pruebas68k/denise.c` (`denise.exe` en git) pone, con `Disable`, seis modos de
    `denise_modos.h` cuatro segundos cada uno, con su lista de Copper (barras de `COLOR00` en seis líneas, 16 `MOVE`
    seguidos en una, un cambio de `COLOR01` a mitad de una línea de la imagen y `BPLCON2` a mitad de pantalla): baja
    con 5 planos, alta con 4, EHB, HAM6, campos dobles con `PF2PRI` desde la línea `$AC`, y baja con 3 planos,
    desplazamiento fino distinto en cada campo, una palabra más de lectura con módulo y otra ventana. La captura no
    sirve con `--screenshot` de `emu_run.py` (la saca `shot.c` de la pantalla de Intuition): se corre FS-UAE con su
    ventana en un Xvfb propio y se pulsa F10 cada segundo con `action_screenshot`
    (`caja/chips/tests/denise_oraculo/captura.sh` y `teclas.py`). Su captura "full" es 752 x 572, sin filtro, con las
    líneas dobladas. `huellas.py` toma los grupos de capturas iguales (un modo cada uno) y guarda la huella de cada
    línea (`huellas.txt`, 26 KB); `tests/denise_oraculo.rs` pinta los mismos modos con los Chips y compara línea a
    línea: **los seis iguales, píxel a píxel**, con las barras, las bandas de los `MOVE` seguidos y el `MOVE`
    retrasado por los planos en alta (quitando ese retraso, la línea `$90` del modo 2 sale distinta). En EHB, el
    A1200 (AGA) parte a la mitad la componente de 8 bits y OCS la de 4: las 71.680 diferencias del modo 3 eran todas
    esa, y `huellas.py` la pasa a la de OCS antes de sacar la huella.
  - Comprobado: `cargo test` (18 pruebas nuevas en `tests/denise.rs` y el oráculo en `tests/denise_oraculo.rs`; las
    de antes siguen, con `direcciones_de_nadie` probando `SPR0POS` en vez de `BPL1DAT`), `cargo clippy --all-targets`
    sin avisos, que compila sin `std` para `x86_64-unknown-none` y `aarch64-unknown-none-softfloat`
    (`-Zbuild-std=core`), y `make -C kernel caja-x86` (BIEN).
  - No comprobado: el oráculo es un A1200 (Lisa), no un OCS; FS-UAE corre con la CPU a "max" (no exacto al ciclo). No
    se probaron en el oráculo: entrelazado, `BPLxDAT` escritos por el 68k, `BPU` = 7, DDF fuera de `$18`-`$D8`,
    cambios de `BPLCON1` o de DDF a mitad de línea, ni ventanas que se abren o cierran a mitad de línea. El robo de
    ciclos a la CPU no se imita. La caja aún no da chip RAM ni recibe la imagen.
  - Dudas, en la cabecera de `denise.rs`: `BPU` = 7 en OCS (aquí lee 4 y pinta 6); `PF2P` mayor que 4; las
    escrituras en `DDFSTRT`, `DDFSTOP`, `BPLCON0` y los punteros llegan a Agnus enseguida (en el Amiga, unos ciclos
    después); los píxeles que caen tras el final de la línea del haz van con su línea.

- Unión de B4 en `chips-blitter` (la sesión), sobre B1-B3. Conflictos en `lib.rs`: B3 y B4 habían dado los dos la
  chip RAM a `avanzar_base`; queda el reparto de B4 (la imagen cuadro a cuadro y después lo demás) y
  `avanzar_sin_pantalla` recibe también la chip RAM, para el DMA de audio. Comprobado: `cargo test` (todas, con el
  oráculo de la imagen) y `cargo clippy --all-targets` sin avisos; `make -C kernel caja-x86` BIEN. No comprobado: el
  audio con los planos a la vez (el DMA de audio no quita ciclos al Copper).
- K6 hecho (papel de implementador; fuentes: el manual de ARMv8-A para la codificación de las cargas y los guardados,
  el código de DashOS y el de Emu68: sus emisores en `M68k_EA.c`, `M68k_MOVE.c`, `M68k_LINE*.c` y `M68k_Endian.h`).
  - La memoria: en la caja de ARM ya no había nada en `$BF0000`-`$BFFFFF` ni en `$DF0000`-`$DFFFFF` (la RAM del 68k
    empieza en `$100000` con 4 MB; lo demás de la caja está por encima de 4 GB), así que no se quita nada: basta la
    regla de K2 de que la RAM no crezca hasta `$BF0000` y que `motor_chips(1)` diga 0 si ya llega.
  - `motor_chips(1)` (`motor_emu68.c`) pone un manejador de fallos (llamada 61) para el hilo de la caja, con una pila de
    64 KB en sus datos; `motor_chips(0)` lo quita (entrada 0). La entrada (`caja_entrada_fallo`, un asm suelto: GCC no
    hace `naked` en AArch64) guarda q0-q31, FPCR y FPSR (el JIT tiene vivos v8-v15 y v19-v31), llama a `chips_fallo` y
    reanuda (62). Atiende solo los fallos de datos con FAR en una de las dos zonas y PC en el código traducido; lo
    demás, y lo que no sabe decodificar, lo dice con `di()`, quita el manejador y reanuda en la misma instrucción: vuelve
    a fallar y el kernel termina el proceso como antes.
  - El decodificador va en `caja/fallo_a64.h`. Cubre las formas que emite el traductor para la memoria del 68k: LDR/STR
    de 1, 2, 4 y 8 bytes con desplazamiento sin signo, sin escalar (LDUR/STUR), pre y post-índice y con registro (UXTW,
    LSL, SXTW, SXTX, escalado o no); LDRSB, LDRSH y LDRSW a W y a X; LDP/STP de W y de X (desplazamiento, pre y
    post-índice, LDNP/STNP) y LDPSW; y las exclusivas de un registro (LDXR, LDAXR, STXR, STLXR, de 1 a 8 bytes; el
    guardado deja el estado a 0). No cubre las de la FPU (FMOVE a un registro de los chips), las atómicas de LSE ni
    LDAR/STLR, que el traductor no emite para esto.
  - Un acceso tiene que caer entero en una zona de 64 KB (la dirección sale de la instrucción, y FAR tiene que estar
    dentro); si cruza a RAM, a la otra zona o a nada, no se parte: acaba como sin chips. Un registro de 8 bytes son dos
    accesos de 4 a los chips y un par son dos registros, siempre el de la dirección menor primero, todos a la misma
    hora.
  - El orden de los bytes: lo que dan los chips se pone en un trozo de memoria con los bytes en el orden del 68k, y se
    carga de ahí en el orden de la caja (con la extensión de signo después); el guardado, al revés. Así el registro
    queda como con la carga de verdad: en big-endian, el valor; en little-endian, los bytes girados, que el REV que
    sigue en el código traducido endereza. La instrucción se lee siempre en little-endian.
  - La hora: `motor_ciclos()` dentro del manejador. El kernel no toca los registros de la FPU al entregar el fallo, y
    la entrada no pisa v20 antes de leerlo, así que es el contador de instrucciones (por 8) como estaba al fallar. Emu68
    lo suma al salir de cada bloque, así que dentro de un bloque la hora se queda en la de su entrada.
  - Prueba nueva en el PC: `caja/pruebas_fallo` (la lanza `make -C kernel prueba`): 83 instrucciones de todas esas
    formas se ejecutan de verdad con qemu-user sobre una memoria en `$BF0000` y se imitan con el decodificador sobre una
    copia; registros y memoria tienen que acabar iguales, en big-endian y en little-endian (20 vueltas al azar). Con un
    fallo metido a propósito (el giro de 16 bits), salen 299 errores.
  - `chips.exe` corre en ARM: fuera "chips: sin chips en este motor" en `arranque_virt.txt`, y las líneas "bien" son las
    mismas en ARM y en x86. Medidas en ARM, en las dos cajas: 3 lecturas del haz por línea (4 en x86), TA tras 15 líneas
    (16 en x86), TOD +1 y +5. Los márgenes de la prueba no se tocaron. `chips.exe` tarda unos 0,4-0,6 s en big-endian y
    de 0,6 a 2,2 s en little-endian (QEMU, no vale como medida).
  - Comprobado: `make -C caja/pruebas_fallo` (BE y LE, 0 fallos); `make -C kernel prueba` (BIEN); `make -C kernel
    caja-x86` (BIEN); `make -C kernel arranque-virt` con la caja big-endian (BIEN a la segunda: la primera falló en el
    guion del selector de guardar, el intermitente de siempre); con `CAJA_ORDEN=le`, BIEN a la tercera (las líneas de
    los chips, bien en las tres; las dos primeras fallaron lejos de la caja, una en el guion de `ajustes` y otra en los
    avisos hostiles del disco RAM, `avisos raros`: sin mirar, apuntado por si se repite).
  - No comprobado: la Pi de verdad; el coste por fallo (eso es K7); FS-UAE (no hacía falta: el binario no cambió).
  - Toca `caja/caja.c` solo en dos comentarios (regla 9: avisar a `dashos-aarch64`). No toca `caja/programas.S` ni
    `caja/Makefile`: `fallo_a64.h` es una cabecera. Toca `kernel/Makefile` (la línea de `pruebas_fallo`) y `caja/motor.h`
    (el comentario de `motor_chips`).

- B5 hecho (papel de implementador; rama `chips-cia`, sale de `chips-blitter`): los CIA completos.
  - Fuentes: el Hardware Reference Manual (el apéndice F y el teclado) y la hoja de datos del 8520, de memoria; vAmiga
    (`Core/Components/CIA`, `Core/Peripherals/Keyboard`, y de mirada `Memory.cpp`, `PaulaRegs.cpp`, `AgnusEvents.cpp`
    y el Blitter; MPL-2.0, cabeceras revisadas), del que salen la tubería de arranque de los temporizadores, la alarma
    por flanco, el registro serie en salida y el saludo del teclado, reescritos con el aviso MPL en `cia.rs` y
    `teclado.rs`. Nada de UAE, WinUAE, FS-UAE (su código), Amiberry ni AROS.
  - El oráculo: `caja/pruebas68k/cia.c` (`cia.exe` en git, no va en la caja) mide con `Disable` y escribe lo que ve.
    Pide a `ciab.resource` los temporizadores del CIA-B (libres con el Kickstart 3.1); las alarmas de los dos CIA las
    tiene el sistema, así que usa la del CIA-B sin pedirla y al acabar la deja poco después del TOD. Se pasó en FS-UAE
    con el A1200 (`--machine a1200`, 68020) y con el A500 (Kickstart 3.1 r40.63 del A500 en `DASH_KICKSTART_A500`,
    compilado aparte con `-m68000`, fuera de git): el A500 es OCS y su 68000 va exacto al ciclo.
  - Las dudas, una a una (lo medido manda donde no coincide con vAmiga):
    - **Retraso de START y de la carga**: medido leyendo el contador cada 2 pulsos E tras arrancar, ocho veces en fases
      distintas. Con `START` empieza a bajar en el 3.º o el 4.º pulso según la fase; con `LOAD` y `START`, igual; con
      el byte alto en una vez, en el 4.º (A500; en el A1200 un pulso más). Se toma vAmiga: 2, 3 y 3 pulsos de espera
      (cabe en lo medido). Con `LOAD` en marcha, FS-UAE carga al momento y sigue bajando (vAmiga espera): manda la
      medida (en el A1200 baja uno menos). Tras desbordar en continuo espera un pulso (periodo `latch + 1`, como antes). Al parar, no baja más
      (vAmiga baja dos; la medida no lo distingue). La cascada (B cuenta desbordes del A) espera un desborde al
      arrancar.
    - **VSYNC y HSYNC**: `VERTB` al principio de la línea 0. El TOD del CIA-A cambia entre la línea 4 (ciclo `$D3`) y
      la 5 (ciclo `$0B`) en el A1200, y entre la 4 (`$C8`) y la 5 (`$25`) en el A500: ahora cuenta al empezar la línea 5
      (`LINEA_VSYNC`), también para su alarma en `siguiente_suceso`. El del CIA-B cambia entre el ciclo `$DE` (A500:
      `$CD`) de una línea y el `$0C` de la siguiente: se queda al empezar cada línea. vAmiga los pone en (5, 84) y en el
      ciclo `$66`, con las lecturas retrasadas.
    - **Alarma del TOD**: por flanco. Escribir el TOD igual a la alarma la hace saltar; escribir la alarma igual al
      TOD, no (A500 y A1200; vAmiga sí). Escribir el byte alto para el TOD hasta el bajo; el del medio no lo para (medido).
      Tras el reinicio el TOD está parado y a 0 (vAmiga; no se puede medir, el Kickstart ya lo puso en marcha).
    - **Registro `$B`**: se lee `$00` en los dos CIA (vAmiga, `$FF`).
    - **Espejos en `$BFxxxx`**: A500 como vAmiga: el bit 12 a 0 elige el CIA-A y el 13 el CIA-B, los dos a la vez si
      los dos están a 0; los bits 0-7 no cuentan; una escritura de 1 byte llega al CIA elegido sea par o impar; uno de
      2 bytes da el alto al B y el bajo al A; la mitad sin CIA se lee `$FF` (de 1 byte, lo que quedó en el bus: `$00`
      o `$21`; aquí `$FF`). Con el A1200, FS-UAE no elige ninguno con los dos bits a 0: aquí se sigue al A500 (OCS).
      Leer la mitad que no se toma también lee el registro (el ICR se borra).
    - **INTREQ desde los CIA**: por nivel (medido: `EXTER` sigue a 1 tras borrarlo sin leer el ICR); como estaba.
    - **`BZERO` tras el arranque**: 0, como vAmiga; no se puede medir desde un programa (en FS-UAE se lee 0 tras el
      arranque, pero el Kickstart ya ha hecho blits).
    - **Bit 14 de INTREQR**: se guarda y se lee (medido: `$4040`); como estaba, y como vAmiga.
    - De paso: el bit 7 de CRA no existe y se lee 0 (medido); el de CRB, sí. El pin CNT y las entradas de los puertos
      quedan como estaban (sin "Duda:"): CNT alto y quieto, entradas a 1.
  - El registro serie: en entrada, cada flanco de subida de CNT mete el nivel de SP y con 8 pasa a `SDR` y sube `SP`;
    en salida, cada desborde del temporizador A es medio bit y `SP` sube en el 16.º desborde contando el que carga el
    byte (FS-UAE: 132-135 pulsos E con latch 7, más cerca de eso que del 15.º de vAmiga). Cambiar de sentido lo
    vacía. PA0 y PA1 del CIA-B no van a SP y CNT (vAmiga sí; FS-UAE no: con ellos en salida no entra nada).
  - El teclado (`src/teclado.rs`): cada código girado e invertido, bit a bit (60 µs por bit, el flanco a los 40), el
    saludo es el CIA-A con el registro serie en salida al menos 85 µs (se cambia con `teclado_saludo_minimo`; vAmiga
    acepta 1 µs), sin saludo en 143 ms resincroniza (un 1 cada 143 ms) y después manda `$F9` y el código perdido; al
    encender espera un segundo a un saludo (como vAmiga) y si no resincroniza, y con el saludo manda `$FD` y `$FE`;
    pila de 10 y `$FA` si se llena. No hace `$78` (reinicio), `$FC` ni las teclas pulsadas al encender entre `$FD` y
    `$FE`. Solo se prueba en Rust: en FS-UAE no se pulsan teclas desde el programa.
  - Los puertos: `Chips::ovl` y `led` (CIA-A PA0 y PA1, por los pines: sin `DDRA` la ROM tapa el 0),
    `fuego(puerto, pulsado)` (PA6 y PA7), `disco_senales(cambio, protegido, pista0, listo)` (PA2-PA5) y
    `disco_lineas()` (los pines de PB del CIA-B, con `paso`, `hacia_dentro`, `cara_arriba`, `elegida(x)` y `motor`).
  - `Chips::tras_el_arranque`: los puertos como los deja el Kickstart (medido), los TOD en marcha y el teclado
    sincronizado. Lo usa `pegamento/src/chips.rs` en vez de `Chips::nuevo`, porque en la caja no hay Kickstart que
    ponga en marcha los TOD (`chips.exe` mira el del CIA-A).
  - Comprobado: `cargo test` (20 pruebas nuevas en `tests/cia.rs`: cada duda con lo medido, el registro serie en los
    dos sentidos, el teclado con saludo, sin él, con uno corto, al encender y con la pila llena, y los puertos; las de
    `tests/chips.rs` puestas al día con los tiempos nuevos), `cargo clippy --all-targets` sin avisos, compila sin `std`
    para `x86_64-unknown-none` y `aarch64-unknown-none-softfloat` (`-Zbuild-std=core`), y `make -C kernel caja-x86`
    (BIEN; `chips.exe` igual: TA tras 16 líneas, TOD +1 y +5, 0 fallos).
  - No comprobado: el teclado contra un Amiga o un emulador (solo en Rust); el retraso de los temporizadores al ciclo
    (FS-UAE da dos valores según la fase del reloj E; aquí no hay fase: cada acceso cae en un pulso entero); los tiempos del registro serie en salida, solo con un
    latch; un Amiga de verdad.


- Unión de B5 en `chips-blitter` (la sesión), sobre B1-B4. Conflictos: la cabecera de `lib.rs`, la prueba
  `direcciones_de_nadie` (B4 usaba `SPR0POS` como registro sin imitar; quedan los espejos nuevos de B5 con
  `SPR0POS`, hasta que lleguen los sprites), el `Makefile` de `pruebas68k` y aquí. Comprobado: `cargo test` (todas)
  y `cargo clippy --all-targets` sin avisos; `make -C kernel caja-x86` BIEN.

### 2026-11-28

- K7 hecho (papel de implementador; fuentes: el manual de ARMv8-A para las codificaciones de `b`, `blr`, LDR literal,
  STR/LDR con pre y post-índice, `dc cvau`/`ic ivau` y NZCV; el código de DashOS y el de Emu68: `M68k_Translator.c`,
  sobre todo `M68K_VerifyUnit`, `M68K_GetTranslationUnit` y dónde suelta las unidades; `support.c` para la limpieza de
  cachés desde EL0). Rama `chips-k7`. No toca Emu68 (ni parche nuevo).
  - El trampolín: uno por sitio, de 64 bytes, en los últimos 64 KB del código traducido (`JIT_LEN`; el tlsf del JIT
    se queda con el resto), así que a menos de 128 MB de cualquier sitio. El sitio pasa a `b trampolín` y el trampolín
    vuelve con `b sitio + 4`: x30 no se pisa fuera. Dentro, guarda x30 en la pila (`str x30, [sp, #-16]!`), salta por
    x30 a una rutina común (`ldr x30, literal; blr x30`: la común está en el texto de la caja, lejos) y al volver
    repone x30. La común (`caja_trampolin_comun`, asm en `caja/trampolin_a64.c`) arma un marco como el de K5 (x0-x30,
    SP, NZCV), llama a `caja_trampolin_c` y lo repone todo, NZCV incluido, con lo que cambió del acceso (el registro
    destino, la base con pre o post-índice, x30 si era el destino). Diseño en la cabecera de `caja/trampolin_a64.h`.
  - Los registros de la FPU y SIMD no se guardan: `trampolin_a64.c` se compila con `-mgeneral-regs-only`, los chips
    son Rust softfloat y `motor_ciclos` solo lee v20. El Makefile de la caja falla si `trampolin_a64.o` llama a
    `memset`/`memcpy` (el de la caja es C normal y puede usar SIMD). Por eso `decodifica` (`fallo_a64.h`) ya no llena
    `struct acceso` con una estructura literal: con musl salía un `memset` que pisaba q0 (lo vio la prueba nueva).
  - La hora: `motor_ciclos()` dentro del trampolín, como en K6 (v20 sigue como estaba). Lo de "cuántas instrucciones
    van en el bloque hasta ese acceso" de S18 queda para cuando el traductor cuente ciclos.
  - Qué se reescribe: todos los sitios que fallan en los chips, con dirección fija o no (en el código traducido toda
    dirección va por registro, no se distingue), con el camino doble: si la dirección no cae entera en una zona de los
    chips, el trampolín vuelve a +20, donde está la instrucción original con los registros como estaban, y hace el
    acceso normal (si cruza a algo sin proyectar, falla ahí y K6 lo trata como siempre). Un sitio que va 1.024 veces
    seguidas a otra cosa vuelve a su instrucción (deja de pagar el rodeo); si vuelve a los chips, falla y se reescribe
    otra vez. No se reescriben las exclusivas (LDXR/STXR: el monitor exclusivo no sobrevive al rodeo; siguen por el
    fallo) ni las de base SP (el trampolín usa la pila).
  - Las unidades que se sueltan: Emu68 las suelta en muchos sitios (`M68K_VerifyUnit` con su CRC del código 68k, sin
    sitio en `M68K_GetTranslationUnit`, CINV/CPUSH en `M68k_LINEF.c`, `M68k_LINE4.c`, `motor_olvidar`). En vez de
    tocarlos todos, un trampolín es de un sitio solo mientras ese sitio salta a él: "deshacer" mira, para cada uno, si
    la palabra del sitio es exactamente `b <ese trampolín>`; si lo es, pone la instrucción original y limpia la caché;
    si no (la memoria ya es de otra unidad), no la toca. Emu68 nunca emite un salto a la zona de los trampolines y los
    datos de control del tlsf no pueden valer `0x14xxxxxx` (punteros por debajo de 4 MB y tamaños), así que no hay
    confusión posible. Los trampolines se reparten seguidos; cuando se acaban los 1.024, se deshace todo y se empieza
    de nuevo (las unidades vivas vuelven a fallar una vez por sitio). Un trampolín no se reutiliza mientras alguien lo
    ejecuta: solo se reparten en el manejador de fallos, y dentro de un trampolín no corre código 68k. El CRC de
    `M68K_VerifyUnit` es del código 68k, no del ARM: reescribir no invalida la unidad (el riesgo de la sección 9 de
    S18 no se da).
  - Se deshace todo en `motor_olvidar` (antes de soltar las unidades; también desde una llamada nativa, donde la unidad
    fijada sigue viva y su sitio vuelve a la instrucción original, que es correcto) y en `motor_chips(0)`: con los
    chips apagados no queda nada reescrito. `motor_chips(1)` pone las cuentas a 0.
  - Escritura del código: por `JIT_RW`, y `dc cvau` + `dsb ish` + `ic ivau` (en `JIT_RX`) + `dsb ish` + `isb`, como
    Emu68; primero el trampolín entero y después la palabra del sitio. Las instrucciones, siempre en little-endian; los
    dos literales (la común y la descripción), en el orden de la caja.
  - Prueba nueva en el PC: `caja/pruebas_fallo/prueba_trampolin.c` (la lanza el mismo Makefile, y con él `make -C
    kernel prueba`). Un arnés pone x1-x8, x30, NZCV y q0-q31 al azar y ejecuta una instrucción en su sitio: primero la
    original, después reescrita por `trampolin_poner`. 52 formas (las de K6 sin las exclusivas, y cinco con x30), 20
    vueltas, con la base en los chips (la de verdad sobre memoria en `$BF0000`, el trampolín sobre la sombra) y en la
    RAM (las dos sobre la misma memoria, por el camino normal); registros, NZCV, memoria y q0-q31 tienen que acabar
    iguales. Además: deshacer devuelve el sitio, 1.024 vueltas a la RAM también, y no se reescriben una exclusiva ni
    una de base SP. En big-endian y en little-endian. Con fallos metidos a propósito: sin reponer NZCV, 1.972 errores;
    con x30 cambiado, 1.040.
  - La medida: `caja/pruebas68k/vposr.s` (`vposr.bin`, en `$138000`) lee VPOSR mil veces por cada carácter de sus
    argumentos. La caja (ARM) lo lanza tres veces con los chips encendidos: sin argumentos (lo que cuesta lanzarlo, que
    se resta), 10.000 lecturas sin reescribir (`chips_reescribe` = 0) y 100.000 reescribiendo. Comprueba las cuentas:
    10.000 fallos y 0 trampolines; luego tantos fallos como sitios reescritos (16: Emu68 desenrolla el bucle) y el
    resto por el trampolín, y 0 por el camino normal. En QEMU (TCG, en este PC; solo para comparar):

    | caja | por el fallo | reescrita | chips.exe |
    |---|---|---|---|
    | big-endian | 22.662 ns | 299 ns | 24 ms (K6: 0,4-0,6 s) |
    | little-endian | 31.365 ns | 353 ns | 30 ms (K6: 0,6-2,2 s) |

    Unas 75-90 veces menos por acceso. Lanzar el programa, de 0,8 a 1,5 ms. En la primera vuelta en little-endian
    (la que falló lejos), 24.895 y 309 ns. Los tiempos de verdad, en la Pi.
  - Comprobado: `make -C caja/pruebas_fallo` (K6 y K7, BE y LE, 0 fallos); `make -C kernel prueba` (BIEN);
    `make -C kernel arranque-virt` con la caja big-endian (BIEN a la primera, con la línea nueva de K7); con
    `CAJA_ORDEN=le`, BIEN a la segunda (la primera, con las líneas de los chips bien, falló lejos de la caja: el
    explorador no ordenó "por Tamano" a tiempo; sin mirar, apuntado por si se repite); `make -C kernel caja-x86` (BIEN). Con objdump: ni `trampolin_a64.o` ni
    `motor_ciclos` usan registros de la FPU o SIMD.
    Nota: la vuelta en big-endian aceptaba hasta 16 sitios reescritos en `vposr`; después se aflojó a 64 (salen 16, el
    desenrollado de Emu68) y con eso corrió la de little-endian.
  - No comprobado: la Pi de verdad (ni los tiempos ni la coherencia de cachés en una A72 real); un sitio que alterna
    chips y RAM en un programa de verdad (solo la prueba del PC); que se llenen los 1.024 trampolines (no pasa en las
    pruebas); el camino de una unidad soltada por Emu68 con su sitio reescrito y la memoria reutilizada (el argumento
    de arriba, sin prueba propia).
  - Toca `caja/caja.c` (la medida K7 tras `chips.exe`, solo en ARM, y las declaraciones de las cuentas y de
    `prog_vposr`), `caja/programas.S` (`vposr.bin`) y `caja/Makefile` (`trampolin_a64.o` con `-mgeneral-regs-only` y
    su comprobación, y `vposr.bin` en `PROGRAMAS_DEPS`). Regla 9: avisar a `dashos-aarch64`. También
    `caja/fallo_a64.h` (sin `memset`), `caja/motor.h` (comentario), `caja/pruebas68k/Makefile`, `caja/pruebas_fallo/` y
    `kernel/pruebas/arranque_virt.txt` (la línea de K7, en un bloque `#[arm`).
  - Para K8: el contador hacia atrás vuelve a los chips desde el bucle de Emu68, no desde aquí; lo que haga falta de la
    hora exacta del acceso (las instrucciones del bloque hasta el sitio) puede ir como un dato más en `struct
    trampolin` cuando el traductor lo sepa.

- Unión de K7 en `chips-blitter` (la sesión), con `main` ya traído (PR #16). Solo chocaron el `Makefile` de
  `pruebas68k` y aquí. Y, pedido por `amiga-en-linux` (la caja como programa de Linux): el decodificador pasa a
  `caja/fallo_a64_decodifica.h` (`marco_fallo`, `struct acceso`, `decodifica`), sin nada de los chips ni del sistema;
  `fallo_a64.h` lo incluye. Comprobado: `make -C caja/pruebas_fallo` (83 y 52 instrucciones, 0 fallos, en los dos
  órdenes), `make -C kernel prueba` BIEN y `make -C kernel caja-x86` BIEN. No repetido tras la unión: `arranque-virt`.

### 2026-11-28

- B6 hecho (papel de implementador; rama `chips-sprites`, sale de `chips-blitter`): los sprites y los choques de OCS.
  - Fuentes: el Hardware Reference Manual (capítulos 4 y 7), de memoria, y vAmiga (`Core/Components/Agnus`:
    `Agnus.cpp`, `AgnusRegs.cpp`, `AgnusDma.cpp`, `Sequencer/SequencerDas.cpp` y `SequencerBpl.cpp`;
    `Core/Components/Denise`: `Denise.cpp`, `Denise.h`, `DeniseRegs.cpp`; cabeceras MPL-2.0 revisadas): los ciclos
    de cada sprite (`$15 + 4n` y el siguiente), qué lee Agnus según `VSTART`/`VSTOP`, la lectura del control en la
    línea 25, el ciclo que se comen los planos (OCS: desde el anterior al primero suyo), el retraso con que Denise ve
    cada registro, el primer píxel en `HSTART + 1`, el recorte desde el primer `BPL1DAT`, el orden de prioridad y los
    choques. Reescrito en Rust, con el aviso MPL y el origen en la cabecera de `sprites.rs`. Nada de UAE, WinUAE,
    FS-UAE (su código), Amiberry ni AROS.
  - Cómo es: los ciclos de sprites van en el bucle de ciclos de `denise.rs` (sin imagen y sin planos, solo los de
    `$15` a `$33`). Lo que llega a Denise (del DMA, del 68k o del Copper) se apunta con la posición en que lo ve (hasta
    128 por línea) y se aplica al traducir la línea; los sprites se mezclan en `traducir` con la prioridad de
    `BPLCON2` (con un campo, `PF2P`; con dos, el del campo que se ve) y allí mismo se calculan los choques. Leer
    `CLXDAT` y escribir `CLXCON` traducen antes lo que ya se ve. `lib.rs`: el módulo, el campo `sprites`, `SPREN` en
    `dma`, `CLXDAT` en las lecturas y `escribir_sprites` en las escrituras. Sin DMA de sprites antes de la línea 25
    (del manual; vAmiga leería en la línea 0 tras una lista acabada).
  - Oráculo: `caja/pruebas68k/sprites.c` (`sprites.exe` en git) pone, con `Disable`, 22 escenas de
    `sprites_escenas.h`: seis quietas cuatro segundos para la captura (baja con 3 planos y `BPLCON2` 0 abajo; `PF2P`
    1 y 3; campos dobles con `PF1P`/`PF2P` y `PF2PRI`; alta con 4 planos; `DDFSTRT = $28`, que se come los sprites 5,
    6 y 7; HAM6) con los ocho sprites, uno reutilizado tres veces (una en la línea siguiente al `VSTOP`), dos unidos,
    `SH0`, sprites que cruzan los cuatro bordes, el Copper moviendo el sprite 1 a mitad de línea (sale dos veces) y el
    sprite 5 a mano (armado por el Copper en la línea `$B0`, desarmado en la `$C0`); y 16 de solo choques (bandas de
    planos con `CLXCON` que miran cada campo, con uno y dos campos; choques entre grupos con y sin `ENSP`; el sprite 6
    solo fuera de la ventana; sin sprites). En cada escena lee `CLXDAT` de un cuadro entero y lo escribe por la
    consola. Capturas con `captura.sh DIR sprites 55` (ahora con el programa y el número de capturas como
    argumentos, la salida en `DIR/salida.txt` y `uae_collision_level=full`) y `huellas.py DIR sprites` (ahora salta
    las capturas de solo borde); en git, `tests/denise_oraculo/sprites_huellas.txt` (26 KB) y
    `sprites_clxdat.txt`. `tests/sprites_oraculo.rs`: **las seis imágenes iguales, píxel a píxel**, y **`CLXDAT`
    igual en las 22 escenas**, con una excepción a la vista: con `CLXCON` sin ningún `ENBP`, FS-UAE da siempre los
    bits 1-8 (también sin ningún sprite, escena 20), y en esas 17 escenas la prueba no compara esos bits.
  - Lo que se vio: el FS-UAE de serie no da nunca el bit 0 (campo con campo): hace falta `uae_collision_level=full`.
    Con un campo, un sprite sobre un píxel que choca con el campo 1 y no con el 2 no da el bit del 1; con campos
    dobles, sí (escenas 7 y 8; como vAmiga). Los sprites que cruzan el borde se cortan justo en la ventana.
  - Comprobado: `cargo test` (20 pruebas nuevas en `tests/sprites.rs`: un sprite y lo que lee Agnus, `SH0`, los
    colores de cada pareja y los ocho en una línea, reutilizar el canal, unidos, el menor delante, prioridad con un
    campo y con dos, HAM y EHB debajo, solo dentro de la ventana, sin planos no se ven, sin `SPREN`, `DDFSTRT`
    temprano con y sin planos en la línea, el modo a mano por el 68k, el Copper moviendo un sprite, choques entre
    sprites y con los campos, `CLXDAT` al encender; y los dos del oráculo; `direcciones_de_nadie` prueba ahora
    `DSKPTH`), `cargo clippy --all-targets` sin avisos, que compila sin `std` para `x86_64-unknown-none` y
    `aarch64-unknown-none-softfloat` (`-Zbuild-std=core`), y `make -C kernel caja-x86` (BIEN).
  - No comprobado: el oráculo es un A1200 (Lisa y Alice), no OCS: no se probó lo que cambia entre los dos (el segundo
    ciclo del sprite 6 con `DDFSTRT = $30`, el del 4 con `$28`). Los bits 1-8 de `CLXDAT` con `CLXCON` sin `ENBP`
    quedan sin decidir (FS-UAE frente al manual y vAmiga). Los bits altos de `VSTART`/`VSTOP` sí salen en el oráculo
    (el sprite 7 de la línea `$118` a la `$136`). No se probaron en el oráculo: sprites con entrelazado, el 68k
    escribiendo los registros de sprites, `CLXDAT` leído a mitad de cuadro, ni las escrituras que vAmiga
    pierde. Sin imagen no hay choques. La caja aún no da chip RAM ni recibe la imagen.
  - Dudas, en la cabecera de `sprites.rs`: la regla de OCS de los ciclos perdidos; la rareza del campo 1 con un
    campo; los bits 1-8 con `CLXCON` sin `ENBP`; sin imagen no hay choques; los unidos chocan con sus propios bits;
    Agnus ve `POS`/`CTL` enseguida; los registros de desplazamiento se vacían en cada línea; `PF1P`/`PF2P` mayor que
    4; más de 128 cambios de sprites en una línea.


- Unión de B6 en `chips-blitter` (la sesión), sobre B1-B5 y K7. Conflictos: la cabecera de `lib.rs`, los módulos,
  la prueba `direcciones_de_nadie` (el registro sin imitar pasa a ser `DSKPTH`, con los espejos de B5), el `Makefile`
  de `pruebas68k` y aquí. Comprobado: `cargo test` (todas, con los oráculos de la imagen y de `CLXDAT`), `cargo
  clippy --all-targets` sin avisos y `make -C kernel caja-x86` BIEN.
