# S16. Un kernel muy seguro: dónde estamos y qué le falta

Pausa pedida por el usuario el 2026-11-03, después de C31: "tenemos que hacer un kernel ultra seguro. Planifica
como prioridad la seguridad. Si hay que cambiar cosas de la arquitectura, estamos a tiempo".

Este documento hace tres cosas. Pone en perspectiva qué debería tener el kernel. Propone los cambios de arquitectura
que conviene hacer ahora, mientras es pequeño. Y ordena el trabajo por fases. La auditoría que lo alimenta está en
`docs/seguridad/auditoria-2026-11-03.md`. Lo que el kernel ofrece hoy está en `docs/referencia/kernel.md`.

## Dónde estamos

El kernel tiene unas 5.900 líneas en `kernel/src/`, con unos 150 usos de `unsafe` repartidos por casi todos los
ficheros.

**Lo que está bien.** El diseño de base es el correcto para un kernel seguro:

- Micronúcleo con capacidades (S1).
- Un proceso nace sin nada.
- Las páginas de usuario cumplen W^X y llegan a cero.
- Las copias de la memoria de usuario respetan sus permisos (C31).
- La FPU no deja ver registros de otro hilo.

**Lo que está mal.** La auditoría encontró 4 fallos críticos y 7 altos. Un programa hostil puede:

- tumbar el kernel con una sola llamada (una llave repetida);
- quedarse con memoria de otro proceso, por la falta de cerrojo en las páginas y por los ASID de 8 bits;
- colgar núcleos con un interbloqueo de canales;
- agotar la memoria, que acaba en pánico.

Nada de esto es raro en un kernel que se construyó en una noche para que las cosas funcionasen. Pero hay que
arreglarlo antes de construir encima.

**Lo que se ha colado en el kernel y no debería estar.** El kernel lleva dentro:

- el USB (391 líneas, que interpretan descriptores que manda cualquier aparato que se enchufe);
- la GPU (V3D, 502 líneas) y el compositor de vídeo (HVS, 237);
- el compositor de pantalla;
- 21 pruebas en `main.rs`, que van en la imagen del kernel.

S1 dice que todo eso va fuera, en procesos. Se metió dentro para avanzar deprisa.

## Qué debe tener un kernel muy seguro

Ocho propiedades. Las cumplen, cada una a su manera, seL4, Fuchsia (Zircon) y QNX.

1. **Ninguna llamada puede tumbarlo.** Cualquier valor en cualquier registro da un error, nunca un pánico. Quedarse
   sin memoria es un error que se devuelve.
2. **Todo recurso tiene dueño y cupo.** La memoria del kernel que gasta un proceso, como tablas, llaves, colas o
   pilas, se carga a su cuenta. Agotarla le hace daño a él, no al sistema.
3. **Nada de autoridad ambiental.** Solo se puede hacer lo que permite una llave. Las llaves se recortan al pasarlas,
   se revocan, y llevan la identidad de quien las da.
4. **La base de confianza es pequeña y se puede revisar.** El kernel solo hace lo que no puede hacer nadie más:
   memoria, planificador, mensajes e interrupciones. El `unsafe` vive en un solo sitio.
5. **El hardware se usa para defenderse:**
   - permisos por sección en el propio kernel;
   - pilas con guarda;
   - el kernel no ve la memoria de usuario salvo cuando copia (PAN);
   - direcciones al azar;
   - defensas contra la ejecución especulativa.
6. **Ningún dato pasa de un proceso a otro sin una llave.** Ni registros del sistema, ni TLB, ni caché de
   traducciones.
7. **El trabajo del kernel está acotado.** Ninguna llamada tarda más de un tope conocido con las interrupciones
   enmascaradas, y no hay esperas activas en cerrojos que se puedan quedar sin dueño.
8. **Se prueba contra un atacante, siempre:**
   - un programa hostil por cada clase de fallo, como `malo.rs`;
   - llamadas con valores al azar en QEMU en cada cambio;
   - la lógica pura del kernel probada en el PC, con herramientas que buscan contraejemplos.

Hoy el kernel cumple a medias la 3 y la 5, y no cumple las demás.

## Cambios de arquitectura que conviene hacer ahora

Son los que costarán más cuanto más tarde se hagan. Cada uno lleva su recomendación. Los decide el usuario.

### A. El kernel también en little-endian

Hoy el kernel corre en big-endian. Desde C30 ningún programa lo ve. Pasarlo a little-endian tiene tres ventajas:

- **Compilador más probado.** El objetivo de Rust pasa a ser `aarch64-unknown-none-softfloat`, de nivel 2, probado en
  cada versión. El big-endian es de nivel 3: nadie garantiza que compile bien, y un fallo del compilador en el kernel
  es un fallo de seguridad.
- **Herramientas de verificación.** Kani y Miri corren en el PC, que es little-endian. La lógica del kernel se podría
  comprobar con ellas sin traducir nada.
- **Menos código raro.** Desaparecen las vueltas de bytes con los periféricos (`RegLe32`) y con el árbol de
  dispositivos.

Coste: el arranque, las tablas de páginas y lo que lee la GPU. Del orden de un día. La caja Amiga no se ve afectada,
porque su orden lo pone `E0E` por proceso.

**Recomendado: sí.**

### B. Todo el `unsafe` en una sola caja

Es la disciplina de Asterinas, el "framekernel" que ya cita S1. Una caja de Rust, `base`, tiene todo el `unsafe`:

- registros del sistema;
- tablas de páginas;
- cambio de contexto;
- copias de usuario;
- acceso a periféricos.

Le da al resto una interfaz segura. El resto del kernel lleva `#![forbid(unsafe_code)]`. Así, revisar la seguridad
del kernel es revisar `base`, que debería quedarse en unos pocos miles de líneas.

**Recomendado: sí.** Mejor ahora, con 150 `unsafe`, que con 1.500.

### C. Sacar del kernel lo que no es del kernel

- **El USB, a un proceso.** Es la mayor superficie de ataque: interpreta lo que manda cualquier aparato que se
  enchufe.
- **La GPU y el compositor de vídeo, a procesos.** La GPU, con su propio servidor (S9).
- **El compositor de pantalla, a un proceso.**
- **La consola del kernel.** Se queda solo para los pánicos, con su propio puerto. Lo que escriben los programas va a
  un servicio de registro.
- **Las pruebas, fuera de la imagen.** Se compilan solo con una opción de pruebas. Las que se puedan pasan a programas
  de usuario.

Para sacar el USB y la GPU hace falta un objeto nuevo: **memoria para DMA**, con su dirección de bus.

**Recomendado: sí**, por este orden: pruebas, consola, USB, compositor, GPU.

### D. Qué hacer con el DMA

En la Pi 4 no hay IOMMU. Un controlador que programa el DMA de su aparato puede escribir en cualquier sitio de la
memoria que alcance el aparato, incluido el kernel. No hay forma de impedirlo solo con software. Las opciones:

1. **Decirlo y reducirlo.** Los controladores con DMA son de confianza: se marcan, se firman, se revisan y van en
   `SYS:`, que no cambia nadie (S6). Los que no usan DMA quedan aislados del todo.
2. **Que el kernel programe el DMA.** El controlador pide "copia este búfer al aparato" y el kernel escribe los
   descriptores. Es seguro, pero el kernel tiene que conocer cada motor de DMA, y vuelve a crecer.

**Recomendado: 1 en la Pi 4, con la memoria DMA del apartado C** para que, al menos, los búferes salgan de objetos con
dueño. En otras placas, comprobar si la CPU tiene IOMMU para los aparatos que se usen, y usarla si la tiene. Es el
caso de la Pi 5, que está por confirmar.

### E. Una sola regla para los cerrojos y el tiempo

Hoy se mezclan dos modelos:

- llamadas al sistema con las IRQ enmascaradas;
- hilos del kernel que se pueden expulsar mientras tienen un cerrojo.

Esa mezcla causa los hallazgos 7 y 10 de la auditoría. Recomendado, como seL4: **el kernel no se expulsa nunca**.

- Toda llamada corre con las IRQ enmascaradas.
- Toda llamada tiene un tope de trabajo. El trabajo largo se trocea en pasos que se pueden reanudar. Por ejemplo,
  poner a cero 64 MB se hace al usar cada página, o por tandas.
- Los cerrojos solo cubren tramos cortos y nunca se toman en orden distinto.
- Los hilos del kernel, como el repartidor o el hilo que recoge, se reducen a lo mínimo, siguen la misma regla o pasan
  a procesos.

**Recomendado: sí.**

### F. Cupos al estilo de los jobs de Fuchsia

Cada proceso pertenece a un grupo con un presupuesto de memoria del kernel, que le da quien lo crea. Todo objeto que
crea se cobra ahí: páginas, tablas de páginas, llaves y mensajes en cola. Al agotarse, la llamada devuelve
`E_SIN_MEMORIA`. Matar el grupo libera todo. Es también la base de "matar siempre funciona" (S2).

**Recomendado: sí.**

### G. Capacidades completas

- **Duplicar una llave con menos derechos.** Así, por ejemplo, se comparte memoria en solo lectura.
- **Revocar.** Quien dio una llave puede retirarla, y la retira de todas sus copias.
- **Identidad del que escribe.** Cada extremo lleva una marca que pone quien lo crea (el "badge" de seL4). Un
  servidor sabe quién le habla sin fiarse del mensaje.
- **Sin ciclos.** No se puede mandar un extremo por su propio canal ni por su pareja. La destrucción de objetos va en
  bucle, con una cola, no por recursión.
- **Al morir un proceso** se cierran sus extremos, y el otro lado se entera.

**Recomendado: sí.**

## Defensas de hardware

La Pi 4 tiene un Cortex-A72 (ARMv8.0). No tiene PAN, PAC, BTI ni MTE. La Pi 5 tiene un Cortex-A76 (ARMv8.2): gana
PAN, pero tampoco tiene PAC, BTI ni MTE.

**En la Pi 4, lo que hay que hacer a mano:**

- **PAN por software.** Mientras el kernel trabaja, TTBR0 apunta a una tabla vacía. Solo apunta a la del proceso
  durante las copias de `copia.s`. Así un puntero perdido del kernel no puede tocar memoria de usuario.
- **Mapa del kernel con permisos por sección.** Código solo lectura y ejecución, datos de solo lectura sin
  ejecución, el resto escribible sin ejecución. Además, todo TTBR1 sin ejecución para EL0.
- **Pilas del kernel con guarda.** Cada pila en su tramo, con una página sin proyectar debajo.
- **Sin la proyección identidad del arranque** una vez encendida la MMU.
- **Kernel en dirección al azar** (KASLR), cuando haya buena entropía.
- **Registros que ve EL0 con valor conocido.** `TPIDR_EL0` se guarda por hilo. `TPIDRRO_EL0`, `CNTKCTL_EL1`,
  `PMUSERENR_EL0` y `MDSCR_EL1` se fijan al arrancar cada núcleo.
- **Ejecución especulativa.** Según la tabla pública de Arm, el A72 es vulnerable a Spectre v1, v2, v4 y BHB. Hay que
  confirmarlo con esa tabla. Las defensas:
  - v1: enmascarar los índices que vienen del usuario antes de usarlos.
  - v2 y BHB: vaciar el predictor de saltos al cambiar de proceso, con la secuencia que Arm da para el A72.
  - v4: activar SSBD si el firmware lo ofrece.

  Son caras. Hay que medirlas, y aplicarlas al menos al cambiar entre procesos de usuarios distintos.
- **Entropía de verdad.** El generador por hardware de la Pi 4 (RNG200) siembra `azar.rs`. Hoy sale del contador.

**En la Pi 5:** PAN de verdad en vez del de software, y lo que tenga de lo demás.

## Cómo se comprueba

- **Un programa hostil por cada clase de fallo**, como `malo.rs`, en la tanda de pruebas. Cada hallazgo de la
  auditoría, al arreglarse, deja su prueba.
- **Fuzzing de llamadas en QEMU.** Un proceso hace millones de llamadas con números, punteros y llaves al azar, desde
  varios núcleos a la vez. El kernel no debe caer ni colgarse. Se lanza en cada cambio del kernel.
- **La lógica pura, en el PC.** La tabla de llaves, los derechos, el repartidor de páginas y los cupos se compilan
  también para el PC. Ahí se prueban con:
  - pruebas de propiedades;
  - Kani, que busca contraejemplos de forma exhaustiva hasta un tamaño;
  - Miri, que detecta comportamiento indefinido.

  El apartado A lo facilita.
- **Auditorías periódicas** con lectura independiente, como la de este documento, después de cada fase.
- **Verificación formal**, a más largo plazo, de las piezas pequeñas que más importan: tabla de llaves, derechos y
  repartidor de páginas. Con Verus o Kani. seL4 demuestra que se puede; no hace falta hacerlo todo.

## Plan por fases

Cada fase se cierra con sus pruebas hostiles y con una auditoría nueva.

**Fase 1. Que ningún programa tumbe ni lea el kernel.** Son errores que un programa puede explotar hoy:

- Hallazgo 6: la llave repetida.
- Hallazgo 1: un cerrojo en el repartidor de páginas.
- Hallazgo 2: ASID de 16 bits con generaciones, y TTBR0 vacío fuera de los procesos.
- Hallazgo 7: el orden de los cerrojos del canal.
- Hallazgo 3: la destrucción en bucle y las pilas con guarda.
- Hallazgo 8: errores en vez de pánicos al quedarse sin memoria, y devolver lo tomado cuando se falla a medias.
- Hallazgo 11: un SError o un FIQ terminan el proceso, no el kernel.
- Hallazgos 5 y 16: registros de EL0.
- Hallazgo 12: TTBR1 sin ejecución para EL0.
- El cargador de ELF y los hallazgos bajos.

No necesita decisiones nuevas.

Hecha el 2026-11-03 como C32 y cerrada con su auditoría (`docs/seguridad/auditoria-2026-11-03-fase1.md`). Lo
que queda de ella está en la bitácora de `docs/CONSTRUCCION_LOOP.md`.

**Fase 2. La base.**

- Kernel en little-endian (A).
- `unsafe` en una caja (B).
- Pruebas fuera de la imagen.
- Regla de no expulsión (E).
- Mapa del kernel por secciones, PAN por software y quitar la proyección identidad.

**Fase 3. Capacidades y cupos.**

- Cupos por grupo (F).
- Recortar, revocar, identidad del emisor, sin ciclos, y cerrar al morir (G).
- Crear y matar procesos desde un proceso.

**Fase 4. Sacar lo que sobra.**

- Memoria DMA (D).
- Consola de registro.
- Sacar el USB, el compositor y la GPU (C).

**Fase 5. Comprobación continua.**

- Fuzzing en cada cambio.
- Lógica pura en el PC con Kani y Miri.
- Defensas contra la ejecución especulativa, medidas.

**Fase 6. En la placa.**

- RNG200 y KASLR.
- Registros del firmware.
- Arranque verificado como opción (S6).
- Repetir las pruebas hostiles en la Pi 4.

## Decisiones

El usuario aprobó las siete el 2026-11-03 ("aprobados"):

1. El kernel pasa también a little-endian (A).
2. Todo el `unsafe` va en una caja; el resto, con `forbid(unsafe_code)` (B).
3. El USB, la GPU, los compositores, la consola y las pruebas salen del kernel (C).
4. En la Pi 4, los controladores con DMA son pieza de confianza, firmados y en `SYS:` (D).
5. El kernel no se expulsa nunca y cada llamada tiene trabajo acotado (E).
6. Cupos por grupo de procesos (F) y capacidades completas (G).
7. Este orden de fases, empezando ya por la fase 1.

## Cómo encaja

- Concreta S1 (micronúcleo, `unsafe` acotado, qué va fuera), S2 (matar siempre funciona), S3 (W^X, direcciones al
  azar) y S6 (otras defensas).
- Sustituye en S6 la frase sobre PAN, PAC, BTI y MTE. La Pi 5 tampoco tiene PAC, BTI ni MTE.
- La referencia (`docs/referencia/kernel.md`) recoge cada cambio cuando se hace.
