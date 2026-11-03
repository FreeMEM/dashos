# Auditoría de seguridad del kernel, 2026-11-03

Hecha por un agente con lectura independiente, sin ejecutar nada, sobre el código de `kernel/src/` tras C31. Los números
de línea son de esa versión. El estado de cada hallazgo y el plan para arreglarlo están en
`docs/diseño/S16-kernel-seguro.md`. Este fichero no se actualiza: es la foto de ese día.

Antes de copiar aquí los críticos y altos (1, 2, 6 y 7), se comprobaron a mano en el código. Los demás son de la
auditoría tal cual.

Modelo de amenaza: un programa hostil en EL0. Puede hacer cualquier llamada con cualquier valor, ejecutar cualquier
instrucción y tener varias llaves. Busca:

- tumbar el kernel;
- leer o escribir memoria que no es suya;
- ganar derechos que no tiene;
- saltarse el aislamiento entre procesos.

También cuenta un controlador en EL0 con llave de registros o de interrupción.

Ya se conocían y no entran: el puntero sin proyectar (arreglado en C31), el mensaje que se pierde en `canal_leer`
(también en C31), la TLB al proyectar encima de algo y los canales que no se cierran al morir un proceso.

## Crítica

### 1. El repartidor de páginas físicas no tiene cerrojo

`memoria.rs:37-49` y `143-176`.

`PAGINAS` es un `static mut` usado a la vez desde los cuatro núcleos: `memoria_crear`, `cerrar`, `tabla_nueva`,
`pagina_nueva`, el hilo ocioso y los controladores del kernel. Pasa aunque el comentario dice "solo desde el núcleo 0".
`monton.rs` toma su propio cerrojo, pero eso no protege a los demás usuarios.

Dos núcleos pueden quedarse con la misma página. También se pueden pisar dos actualizaciones (`|=`) de la misma palabra
del mapa, y entonces una página en uso consta como libre.

Escenario: dos procesos en núcleos distintos repiten `memoria_crear` (64 MB) y `cerrar`. Al final, una página es a la vez
memoria del atacante y una tabla de páginas de otro proceso, o del montón del kernel. Desde ahí, escribir descriptores
le da cualquier dirección física.

Comprobado: falta el cerrojo. La carrera es probabilística y no se ha provocado.

### 2. Los ASID son de 8 bits y se repiten sin control

`arranque.s:50` y `espacio.rs:72`.

El valor de `TCR` (`0x4B5193519`) tiene el bit AS (36) a 0, así que el hardware solo usa 8 bits de ASID. `espacio.rs` los
reparte con `& 0xffff`, sin generaciones y sin vaciar la TLB al dar la vuelta.

Escenario: después de 256 procesos, uno nuevo comparte ASID con uno vivo, por ejemplo el controlador serie, y los dos
corren en el mismo núcleo. Las entradas de la TLB del viejo valen para el nuevo. Como los dos ET_EXEC usan las mismas
direcciones, el nuevo lee y escribe las páginas del otro.

Comprobado leyendo el valor de TCR. No se ha probado. Con AS=1 pasaría lo mismo a los 65 536 procesos.

### 3. Un drop recursivo desborda la pila del kernel

`objetos.rs` (la cadena Canal → cola → Mensaje → Llave → `Arc<Canal>`) y `planificador.rs:21,156`.

Las pilas del kernel miden 16 KB, salen del montón y no tienen página de guarda.

Escenario:

1. El atacante crea N canales.
2. Mete los dos extremos de Cᵢ en un mensaje por Cᵢ₋₁.
3. Cierra los dos extremos de C1.
4. La destrucción recorre toda la cadena por recursión, dentro de la llamada y con las IRQ enmascaradas.

Unas decenas de canales deberían bastar para pasar de 16 KB. Lo que hay debajo de la pila puede ser otra pila, una tabla
de páginas o memoria de usuario. Al morir el proceso pasa lo mismo en `recoger()`, sobre la pila de arranque.

Comprobado: la recursión no tiene límite y no hay guarda. No se ha medido la profundidad. Como mínimo tumba el kernel; es
plausible que también sirva para tomarlo.

### 4. Un controlador puede usar el DMA del aparato

`main.rs:702`.

El controlador de SD recibe los registros de EMMC2, un SDHCI con SDMA y ADMA2. EMMC2 llega al primer GB, donde está el
kernel, y la Pi 4 no tiene IOMMU. Un controlador hostil programa una transferencia y reescribe el kernel.

Comprobado: la llave se entrega. Que el aparato pueda hacer ese DMA sale de la especificación SDHCI; no se ha probado.

## Alta

### 5. `TPIDR_EL0` no se guarda por hilo

`TPIDRRO_EL0` tampoco se pone nunca a un valor conocido.

Un proceso lee el puntero de TLS de otro del mismo núcleo, y con él su base, aunque esté puesta al azar. Además es un
canal encubierto. También puede cambiárselo, y el otro hará sus accesos de TLS donde él quiera.

Comprobado. Hoy no lo usa nadie, pero el runtime de Dash lo usará.

### 6. Una llave repetida en `canal_escribir` provoca un pánico

`llamadas.rs:158-166`.

El kernel valida cada número con `ver()` y después hace `quitar().unwrap()`. Con la tabla `[3, 3]`, el segundo `quitar`
devuelve `None` y el `unwrap` aborta el kernel.

Comprobado.

### 7. `Canal::leer` puede interbloquearse

`objetos.rs:119-123`.

`leer` toma el cerrojo de su lado y, sin soltarlo, toma el del otro. Si dos hilos leen a la vez en los dos extremos de
un canal, en núcleos distintos, cada uno espera al otro para siempre. Están con las IRQ enmascaradas, así que se cuelgan
los dos núcleos. Es el patrón normal de pregunta y respuesta.

Comprobado el orden de los cerrojos. No se ha reproducido.

### 8. Quedarse sin memoria acaba en pánico

No hay cupos en ningún sitio:

- `memoria_crear`: hasta 64 MB por llamada, sin límite de llamadas;
- las colas de canal;
- la tabla de llaves;
- `proyectados`, que crece en cada proyección;
- las tablas de páginas.

Cuando se acaba la memoria, `espacio.rs` hace `expect`, o el montón devuelve nulo, y el kernel cae. Unas 30 llamadas a
`memoria_crear` de 64 MB bastan en QEMU, que tiene 2 GB.

Además, si `Memoria::nueva` falla a medias, las páginas que ya había tomado no se devuelven (`objetos.rs:52`).

Comprobado.

### 9. La consola del kernel la comparte un controlador de EL0

El controlador serie recibe los registros del mismo PL011 que usa `kprintln`. Además, `serie.rs:30` espera sin límite a
que haya hueco para enviar, con el cerrojo de la consola tomado y las IRQ enmascaradas.

Escenario: el controlador apaga la UART y después cualquier proceso provoca un `kprintln`, por ejemplo con una llamada
desconocida. Ese núcleo se cuelga, y los demás se cuelgan al pedir el cerrojo.

Comprobado leyendo.

### 10. Hay cerrojos de espera activa en hilos que se pueden expulsar

`Cerrojo` no enmascara las IRQ. El hilo ocioso (en `recoger`) y el repartidor de interrupciones toman el cerrojo del
montón y los de canal con las IRQ habilitadas.

Si el tic expulsa a uno de ellos con el cerrojo tomado, y entra un proceso del mismo núcleo que reserva memoria, ese
proceso gira para siempre con las IRQ enmascaradas. El dueño del cerrojo no vuelve a correr.

Comprobado el mecanismo. Que ocurra depende de la suerte.

### 11. Un SError o un FIQ desde EL0 tumban el kernel

`excepciones.rs:71-120`: los tipos 10 y 11 caen en el `panic!` final.

Escenario: un controlador escribe en un registro que responde con error externo.

Comprobado el camino. No se ha probado en placa.

## Media

### 12. EL0 puede ejecutar la RAM a través del mapa lineal del kernel

`arranque.s:96`.

Los bloques llevan `0x705`: AP=00, sin UXN ni PXN. En ARMv8 eso es "solo ejecución" para EL0, y el A72 no tiene E0PD.
Un proceso puede ejecutar como EL0 el kernel o las páginas de otros.

Además, todo el mapa lineal se puede escribir y ejecutar desde EL1: no hay W^X en el kernel.

Comprobado leyendo.

### 13. TTBR0 queda mal fuera de los procesos

La proyección identidad del arranque es global y nunca se quita. Al morir un proceso, el núcleo sigue con TTBR0
apuntando a tablas ya liberadas. Un recorrido especulativo podría cargar entradas viejas con un ASID que hereda otro
proceso.

Plausible. No está confirmado.

### 14. Hay llamadas largas con las IRQ enmascaradas

- `memoria_crear` de 64 MB pone a cero 16 384 páginas, y busca cada una recorriendo el mapa.
- `escribir` saca 4096 bytes por la serie, byte a byte.
- `instalar` recorre toda la tabla de llaves.

Rompen la clase de plazo. Comprobado leyendo; no se ha medido.

### 15. Hay fugas permanentes por ciclos

- Un extremo de canal mandado por su pareja: el canal queda dentro de su propia cola.
- Una llave de interrupción mandada al canal al que está asociada.

Comprobado.

### 16. Hay registros del sistema sin valor conocido

`CNTKCTL_EL1`, `PMUSERENR_EL0` y `MDSCR_EL1`. En QEMU arrancan a 0, pero en la Pi dependen del firmware. Si EL0 tiene
acceso al temporizador físico, un proceso apaga el tic de su núcleo y nadie lo expulsa.

Plausible. No se ha comprobado en placa.

### 17. Los derechos no se pueden recortar

S1 lo promete, pero no hay llamada para hacerlo. Las llaves nacen con todos los derechos.

Comprobado.

## Baja

- **ELF.**
  - `phoff + i * phentsize` puede dar la vuelta.
  - La base más `p_vaddr` puede dar la vuelta.
  - Un segmento W+X se carga como R+X sin avisar.
  - Un `memsz` enorme acaba en `expect`.
  - No se comprueban `e_phentsize`, la entrada ni los solapes.
- **Dirección 0.** Se puede proyectar en ella; sin PAN, un puntero nulo que siga el kernel leería datos del usuario.
- **Interrupciones.** `interrupcion_asociar` y `interrupcion_terminar` no miran los derechos de la llave de interrupción.
- **"IRQ sin dueño".** Se imprime desde la interrupción con un cerrojo que no enmascara IRQ.
- **Registros de aparatos.** Si no están alineados a página, se proyectan también los vecinos.
- **GIC.** `gic.rs` lee, modifica y escribe la prioridad y el destino sin cerrojo.
- **`escribir`.** Deja pasar secuencias de escape y textos que imitan mensajes del kernel.
- **Placas de 8 GB.** Las páginas se reparten desde arriba, pero el mapa lineal solo cubre 4 GB.

## Revisado y correcto

- La precedencia de `&&` y `||` en `memoria_proyectar`.
- W^X y PXN en las páginas de usuario.
- Toda página que llega al usuario se pone a cero.
- x0 a x30 se limpian al entrar en EL0, y una llamada solo cambia x0 y x1.
- La FPU perezosa no deja ver registros de otro hilo.
- E0E se pone en cada cambio de hilo.
- Las copias de C31 respetan los permisos de EL0.
