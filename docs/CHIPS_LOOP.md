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
- [ ] K2 **La interfaz.** En `motor.h`, lo que la caja da al motor: `caja_chip_leer(dir, tam, hora)` y
  `caja_chip_escribir(dir, valor, tam, hora)`. Y lo que el motor da: `motor_ciclos()`, para saber la hora fuera de un
  acceso. Las funciones en C de `pegamento/` llaman a `caja/chips`. Los chips, apagados de serie: se encienden por
  programa (en la cabecera, o una llamada de la caja) o en las pruebas. Sin ellos, la caja sigue como hoy: un acceso
  ahí es un fallo (el Enforcer).
- [ ] K3 **RelevoJIT (x86).** Con los chips encendidos, `motor_relevo.c` proyecta `$BF0000`-`$BFFFFF` y
  `$DF0000`-`$DFFFFF` como `RELEVO_PAGE_HOST`, y sus `read`/`write` llaman a K2 con la hora de `relevo_cycles`.
  Una prueba 68k nueva, `caja/pruebas68k/chips.c`: lee `VPOSR` hasta que cambia de línea, pone en marcha el
  temporizador A del CIA-A una vez, espera a su bit de ICR y lee el TOD. Escribe los sucesos en orden, con
  "bien" o "MAL" en cada uno. Va en `caja-x86`. Se miran el intérprete y el JIT, porque el JIT solo estima los
  ciclos.
- [ ] K4 **Oráculo.** `chips.exe`, el mismo binario, en FS-UAE con el Kickstart del usuario: las mismas líneas.
  Lo que dependa de tiempos se escribe como orden o como un margen (un temporizador de 709 tics da de 14 a 17
  líneas), no como número exacto. Con el sistema parado mientras mide (`Disable`), y usando temporizadores que el
  Kickstart no tenga en uso.
- [ ] K5 **El kernel entrega el fallo al proceso** (ARM primero, x86 después). Una llamada nueva para registrar un
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
