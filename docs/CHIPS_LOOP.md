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
- [ ] K7 **Reescritura (ARM).** Tras el primer fallo en un sitio, la instrucción pasa a un `bl` a un trampolín
  (S18, "Los siguientes: la instrucción reescrita"). Se mide el coste por acceso, con fallo y con trampolín, en QEMU
  (solo para comparar; los tiempos de verdad, en la Pi).
- [ ] K8 **Contador hacia atrás e interrupciones.** El motor vuelve a los chips cuando llega la hora del siguiente
  suceso, y los chips ponen el `IPL`. En la caja nativa, los niveles 6 y 7 ya los usa la caja: aquí solo se dejan
  preparados para la máquina entera (fase 2), sin entregarlos todavía.

Lo que no es de la fase 1: el Copper, el Blitter, Denise, Paula, el disco, la máquina entera y AGA (fases 2 a 7
de S18).

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
