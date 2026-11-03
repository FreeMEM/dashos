# Auditoría de la fase 1 de S16, 2026-11-03

Hecha por un agente con lectura independiente, sin ejecutar nada. Mira los commits de C32 (`744c606..2951c94`) y
vuelve a leer `kernel/src/` entero. Los números de línea son de `2951c94`. Este fichero no se actualiza: es la foto de
ese día.

Modelo de amenaza: el mismo que la auditoría anterior. Un programa hostil en EL0 que hace cualquier llamada con
cualquier valor, ejecuta cualquier instrucción y tiene varias llaves. También un controlador en EL0 con llave de
registros o de interrupción.

"Comprobado" quiere decir comprobado leyendo el código. Nada se ha ejecutado.

## Estado de la fase 1

| Hallazgo | Estado | Dónde |
|---|---|---|
| 6. Llave repetida | Arreglado | `llamadas.rs:192-205`. El `unreachable!()` de la línea 204 es de verdad inalcanzable: los números se validan y se quitan bajo el mismo cerrojo y sin repetidos. |
| 1. Repartidor sin cerrojo | Arreglado | `memoria.rs:46-60`, `monton.rs:26`. Ya no queda ningún `static mut`. Todos los usos pasan por `paginas()`. |
| 2. ASID de 8 bits | Arreglado | `arranque.s:50` (AS = 1), `espacio.rs:13-31` y `73-84`. No hay generaciones, pero el mapa de bits da lo mismo: un ASID no vuelve hasta después de su `tlbi aside1is`. |
| 13. TTBR0 fuera de los procesos (va con el 2) | Arreglado | `espacio.rs:35-37` y `240`, `planificador.rs:156-159`, `413-417` y `534-536`. La proyección identidad sigue en `__tabla_l1` para los núcleos que arrancan (fase 2, a propósito). |
| 7. Orden de cerrojos del canal | Arreglado | `objetos.rs:91-93`. Un cerrojo por canal. No he encontrado ningún orden inverso (ver "Revisado y correcto"). |
| 3. Destrucción recursiva y guardas | El bucle, arreglado. La guarda, a medias | `objetos.rs:107-134` ya no recurre. La "guarda" es una marca (`planificador.rs:186-191` y `430-435`): solo se mira al cambiar de hilo, después de que el desbordamiento ya ha pisado memoria. La página de guarda está aplazada a la fase 2 a propósito. |
| 8. Sin memoria acaba en pánico | A medias | `Memoria::nueva`, `Espacio::nuevo`, `proyectar` y la reserva de 32 MB están bien. Pero un solo proceso puede comerse el montón del kernel con canales anidados. Ver N1. |
| 11. SError y FIQ | Arreglado para el kernel, con un efecto nuevo | `contexto.s:47-48` (F en EL0), `excepciones.rs:70-82`. El kernel ya no cae. Pero el SError puede terminar a otro proceso. Ver N3. |
| 5. TPIDR_EL0 | Arreglado | `planificador.rs:57` y `436-439`; `TPIDRRO_EL0` a 0 en `planificador.rs:176`. |
| 16. Registros sin valor conocido | Arreglado | `planificador.rs:170-182`, en cada núcleo (`main.rs:205`, `nucleos.rs:46`). |
| 12. EL0 ejecuta el mapa lineal | Arreglado | `arranque.s:96`: UXN en los bloques normales. Los de dispositivo ya lo tenían. El W^X del kernel es de la fase 2. |
| ELF | Arreglado | `elf.rs:121-127` (cabeceras), `135`, `148` y `160-163` (sumas comprobadas), `167` (W+X), `197` (entrada). Los solapes se detectan por página, con `Error::Ocupada`. |
| Bajo: dirección 0 | Arreglado | `espacio.rs:99`, `156` y `181`: nada por debajo de 64 KB. |
| Bajo: derechos de la interrupción | Arreglado | `llamadas.rs:362` y `373`. Como todas las llaves nacen con todos los derechos (hallazgo 17), hoy no cambia nada en la práctica. |
| Bajo: "IRQ sin dueño" con cerrojo sin IRQ | Arreglado | `serie.rs:59`. |
| Bajo: registros no alineados a página | No arreglado | Ver N5. |
| Bajo: GIC sin cerrojo | Arreglado | `gic.rs:11`, `62` y `78`. |
| Bajo: `escribir` | A medias | Los escapes, arreglados (`llamadas.rs:144-151`). Imitar mensajes del kernel, no. Ver N6. |
| Bajo: placas de 8 GB | Arreglado | `memoria.rs:92` y `107`. |
| 9. Consola compartida (no estaba en la lista, la bitácora lo da por hecho) | A medias | Ya no espera para siempre, pero sigue colgando los núcleos durante mucho tiempo. Ver N2. |
| 10. Cerrojos expulsables (no estaba en la lista; la bitácora arregla una parte) | A medias, aplazado | El montón y las páginas ya enmascaran las IRQ. Quedan el cerrojo del canal (`objetos.rs:92`), `destino` y `DUENOS` (`interrupciones.rs:30` y `36`). El repartidor los toma con las IRQ habilitadas (`interrupciones.rs:142-148`). Si el tic lo expulsa dentro de `Canal::escribir`, un proceso del núcleo 0 que use ese canal gira para siempre con las IRQ enmascaradas. No es peor que antes. Entra en la regla de no expulsión de la fase 2. |

## Fallos nuevos

### Alta

#### N1. Un solo proceso agota el montón del kernel con canales anidados

`objetos.rs:138` (`Arc::new` en `Canal::nuevo`), `objetos.rs:154-160` (el único límite es la cola de cada lado) y
`monton.rs:42` (el montón pide páginas con `pagina(Zona::Normal)`, que no respeta la reserva).

Los límites de C32 cuentan las llaves de la tabla y los mensajes de cada cola. No cuentan los canales que viven fuera
de la tabla, dentro de los mensajes de otros canales. Esos canales, sus colas y sus mensajes salen del montón del
kernel, y el montón puede comerse también los 32 MB de reserva.

Escenario:

1. El proceso crea el canal A y llena sus dos colas: 16 mensajes de 256 bytes en cada lado.
2. Crea el canal B y manda los dos extremos de A en un mensaje por B. A sale de su tabla.
3. Llena el resto de las colas de B y repite con C, D...

La tabla nunca pasa de unas pocas llaves. Cada canal lleno ocupa unos 26 KB del montón. Con 2 GB bastan unos 80 000
canales, es decir, unos 2,7 millones de llamadas. El paso 24 de `malo` ya hace esto con 3000 canales vacíos; solo le
falta llegar más lejos.

Qué pasa:

1. Primero se acaba la memoria de los demás procesos: la reserva de `pagina_usuario` ya está gastada por el montón.
2. Después, una reserva del montón que no admite fallo recibe un nulo y el kernel entra en pánico. Hay varias al
   alcance de EL0:
   - el `Arc::new` de `canal_crear` (`objetos.rs:138`);
   - los `push` de `canal_escribir` y `canal_leer` (`llamadas.rs:205`, `262-263`);
   - el `push_front` de `Canal::devolver` (`objetos.rs:187`), que puede tener que crecer;
   - el `Box` de `fp_atrapado` (`planificador.rs:609-612`);
   - y en cualquier núcleo, `listos.push` y `muertos.push` del planificador.
3. Con el ciclo del hallazgo 15 (un extremo dentro de su propia cola), todo esto sobrevive a la muerte del proceso.
4. Sin el ciclo, al morir el proceso, `Canal::drop` suelta millones de mensajes con las IRQ enmascaradas (hallazgo 14).

Contradice `docs/referencia/kernel.md:240-241` y la bitácora ("con un proceso solo no se llega").

Comprobado leyendo. No ejecutado.

Arreglo posible: contar por proceso los canales que crea y los bytes que tiene en colas, y sacar las páginas del
montón para objetos de usuario de la misma cuenta que `pagina_usuario`. Hasta los cupos de la fase 3, basta un límite
de canales vivos por proceso que se descuente al soltar el canal.

#### N2. La consola sigue colgando los núcleos, ahora durante minutos

`serie.rs:32-41`, con el cerrojo de `serie.rs:59-62`.

El plazo es por byte, no por llamada. Cada byte espera hasta 200 000 lecturas de `FR`. Mientras, el cerrojo de la
consola está tomado y las IRQ enmascaradas.

Escenario:

1. El controlador serie tiene la llave de los registros del PL011 de la consola (`main.rs:686-689`). Llena la FIFO y
   pone `CR = 0`. `TXFF` se queda a 1.
2. Cualquier proceso llama a `escribir` con 4096 bytes (`llamadas.rs:135-152`), o a una llamada desconocida en bucle.
3. Cada byte agota su plazo.

Con unos 100 ns por lectura de un registro, cada byte tarda unos 20 ms y una llamada de 4096 bytes, más de 80 s. Ese
núcleo no atiende interrupciones en todo ese tiempo. Los demás núcleos se paran igual en cuanto imprimen algo: una
salida de proceso, un fallo o una llamada desconocida. Si el atacante repite, el sistema no vuelve.

Comprobado el mecanismo. Los tiempos son una estimación; no están medidos.

Arreglo posible: después del primer plazo agotado, dar la consola por muerta (un atómico) y no esperar más hasta que
`FR` vuelva a tener hueco. O un plazo por llamada a `imprimir`, no por byte.

### Media

#### N3. Un SError se le cobra al proceso equivocado

`excepciones.rs:79-82`. En EL1, A está siempre enmascarado (`arranque.s:32`, `SPSR_EL2 = 0x3c5`; `reloj.rs:70` solo
abre I).

El SError que provoca un proceso queda pendiente en EL1 y se toma en el siguiente `eret` a EL0, sea del proceso que
sea.

Escenario:

1. Un controlador hostil escribe en un registro que responde con error.
2. Justo después llama a `canal_leer` sobre un canal vacío y se bloquea.
3. El planificador pasa a otro proceso del mismo núcleo.
4. Al bajar a EL0, el SError se toma allí y se termina ese otro proceso.

Así el controlador puede matar procesos ajenos de su núcleo cuando quiera. Los SError de los controladores que siguen
en el kernel (USB, V3D) también terminarían al proceso que toque.

Plausible. No comprobado en placa: QEMU no genera SError en `raspi4b`.

Arreglo posible: antes de cambiar de hilo, y al entrar desde EL0, hacer `dsb sy` y abrir A un instante para recoger
el SError pendiente y cargárselo al hilo que corría.

### Baja

#### N4. Una IRQ en `entrar_el0` deja el hilo en un bucle en EL1

`contexto.s:44-80`. No es de C32. Viene de C11 y la primera auditoría no lo vio.

`entrar_el0` se llama con las IRQ habilitadas (`hilo_estreno` las abre). Si llega una IRQ entre `msr elr_el1` y
`eret`, la excepción pisa ELR y SPSR con la dirección de dentro de `entrar_el0` y modo EL1h. La vuelta de la IRQ los
repone así, y el `eret` final salta otra vez dentro de `entrar_el0`, en EL1. El hilo da vueltas para siempre. El
proceso no arranca nunca y su memoria no se libera.

La ventana es de unas 35 instrucciones por arranque, y EL0 no la controla. Comprobado leyendo.

Arreglo: `msr daifset, #2` al principio de `entrar_el0`.

#### N5. Los registros no alineados dan los aparatos vecinos

`main.rs:686` y `708` redondean el tamaño a páginas, y `llamadas.rs:345-349` proyecta páginas enteras.

El PL011 de la consola mide 0x200 bytes y su página llega a `0xfe201fff`. Según el mapa de la BCM2711, en esa página
están también las UART 2 a 5 (`0xfe201400` a `0xfe201a00`). El controlador serie las recibe sin pedirlas.

Comprobado el redondeo. El mapa de la BCM2711 no lo he comprobado aquí.

Es el bajo de la auditoría anterior, que la fase 1 no tocó.

#### N6. Un programa puede imitar mensajes del kernel

`llamadas.rs:146` deja pasar el salto de línea. Un programa puede escribir líneas enteras como "PÁNICO: ..." o
"C32: ... bien". `tools/prueba_qemu.sh` juzga las pruebas por esas líneas, así que un programa puede falsear el
resultado de una prueba.

Comprobado.

Arreglo posible: poner delante de cada línea de `escribir` un prefijo que el kernel no use, por ejemplo el número de
hilo.

#### N7. Supuestos de un hilo por proceso que se romperán en la fase 3

Hoy no se alcanzan: cada proceso tiene un solo hilo (`docs/referencia/kernel.md:243`). Los apunto para cuando haya
hilos o procesos creados desde un proceso.

- Cada proceso tiene un solo `Arc::into_raw` (`main.rs:637-639`), y `terminar_actual` lo suelta
  (`planificador.rs:530-536`). Con dos hilos sobre el mismo puntero, el segundo usaría el proceso ya liberado.
- `objetos::proceso_actual()` devuelve una referencia sin vida acotada (`objetos.rs:262-265`).
- En `canal_escribir`, entre `quitar` y `reponer` se suelta el cerrojo de la tabla (`llamadas.rs:203-215`). Si otro
  hilo ocupa el hueco, `reponer` tira la llave sin decir nada (`objetos.rs:244-248`).
- Dos lectores en el mismo lado de un canal se pisan `esperando` (`objetos.rs:179`), y uno no despierta nunca.

## Revisado y correcto

- **Orden de los cerrojos.** Lo que he encontrado: montón → páginas; núcleo → montón → páginas (los `push` del
  planificador y `crear_en`); canal → núcleo (`actual()` y `bloquear` en `Canal::leer`); canal → montón; tabla de
  llaves → montón, páginas y GIC (al soltar una llave); espacio → páginas y montón; `destino` → montón. No hay ninguno
  al revés. `despertar` siempre se llama con el cerrojo del canal ya suelto.
- **Anidar `GuardaIrq`.** Siempre se sueltan en orden inverso, así que nunca se reabren las IRQ con un cerrojo
  tomado.
- **Temporales.** No queda ningún `match paginas()...` ni `if let` que tenga el repartidor tomado mientras se reserva
  memoria (el cambio de `gpu.rs` era eso). El `if` de `Interrupcion::terminar` suelta `destino` antes de llamar al GIC.
- **La pista del repartidor** (`memoria.rs:186-244`). `pagina` mira el bit antes de dar una página, así que un error
  en la pista nunca da una página ocupada; como mucho, un `None` de más. El invariante se mantiene:
  - `liberar` sube la pista a la palabra más alta liberada;
  - `marcar_tramo(false)` la pone arriba del todo;
  - `reservar` solo ocupa;
  - por encima de `max_pagina` todo consta lleno.

  La única excepción es la página parcial del final de un tramo de memoria que no acabe en página. Queda por encima del
  tope y nunca se reparte. Solo descuadra en uno la cuenta de `libres`.
- **El mapa de ASID.** El 0 nunca se da. Un ASID vuelve al mapa solo después de su `tlbi aside1is`. Ningún núcleo tiene
  el espacio activo cuando se suelta:
  - `terminar_actual` pasa antes a la tabla vacía;
  - el error de `lanzar_elf` y `prueba_asid` nunca lo activan.
- **`terminar_actual`.** No he encontrado uso después de liberar:
  - El hilo pone `proceso = 0` y `ttbr0 = 0` antes de soltar.
  - Sigue en su propia pila, que solo libera `recoger` desde otro hilo, después del cambio.
  - Deja de ser dueño de la FPU.
  - No tiene ningún cerrojo tomado mientras se suelta el proceso.
  - Nadie más usa ese proceso (un hilo por proceso, ver N7).
  - `recoger` al entrar en cada llamada no puede ver al hilo actual en `muertos`.
- **`Drop` de `Canal`.** Cubre los extremos y las interrupciones con destino. La profundidad es de dos marcos como
  mucho. Hay un caso teórico: si dos núcleos sueltan a la vez las dos últimas referencias de cada nivel, se suma un
  marco por nivel. Haría falta coincidir en cada nivel; no lo veo explotable.
- **`esperando`.** Solo lo pone el lector bajo el cerrojo, y solo lo quita quien lo despierta. Nada más despierta a un
  hilo de usuario bloqueado, así que el puntero no puede quedar colgando de un hilo muerto.
- **Proyecciones.** `preparar` y `proyectar` van bajo el cerrojo del espacio. Después de `preparar`, `proyectar` no
  puede fallar, así que el `let _ =` de `llamadas.rs:326` y `347` es correcto. Los tramos se comprueban con sumas
  comprobadas.
- **`Memoria::nueva`.** Si falla a medias, el `Drop` del objeto parcial devuelve las páginas.
- **Límite de la tabla.** `instalar` y `huecos` coinciden en 1024 llaves.
- **Pánicos al alcance de EL0**, aparte de N1:
  - los `plazo.unwrap()` de `planificador.rs:335-336` están filtrados;
  - los `try_into().unwrap()` de `elf.rs` tienen el tamaño justo;
  - los índices de `excepciones.rs` van enmascarados.

  El perfil `release` no comprueba desbordamientos, y las sumas con valores del usuario usan `checked_*`.
- **Registros de EL0.** `SCTLR_EL1.UMA = 0`: EL0 no puede enmascarar las IRQ. Las FIQ van enmascaradas también en EL1,
  así que la rama de FIQ no se alcanza. `entrar_el0` deja `SPSR` con solo F.
- **Copias de C31.** Siguen correctas: el tramo se mira con `tramo`, y los fallos solo se aceptan dentro de
  `copia_inicio..copia_fin`.
- **USB.** Fuera del modelo de amenaza, porque es un aparato y no un programa, pero lo he mirado: el análisis de
  descriptores de `usb.rs:364-377` comprueba los límites antes de cada índice.
