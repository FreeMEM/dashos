# Auditoría de la fase 2 de S16, 2026-11-03

Hecha por un agente con lectura independiente. No se ha ejecutado QEMU ni se ha cambiado código. Mira los commits C33
a C37 (`6b7fa78..551fbad`) y lee `kernel/src/base/` entero. Los números de línea son de `551fbad`. Este fichero no se
actualiza: es la foto de ese día.

Modelo de amenaza: el de las auditorías anteriores. Un programa hostil en EL0, con cualquier llamada, cualquier valor,
cualquier instrucción y varias llaves. Un controlador en EL0 con llave de registros o de interrupción. Y, para la
decisión B, el resto del kernel: código seguro que llama a las funciones públicas de `base` con argumentos
cualesquiera y en cualquier orden.

"Comprobado" quiere decir comprobado leyendo el código. "Plausible" quiere decir que el camino existe pero no lo he
seguido hasta el final o depende del hardware.

Una aclaración que cambia el peso de todo lo demás: la imagen normal no crea ningún proceso. Sin programa de inicio,
nada baja a EL0. Lo que sigue sobre EL0 se ve hoy solo en la imagen de pruebas, y será el camino del programa de
inicio cuando llegue.

## Estado de la fase 2

| Parte | Estado | Dónde y por qué |
|---|---|---|
| A. Kernel little-endian | Bien | `.cargo/config.toml` (objetivo `aarch64-unknown-none-softfloat`). `arranque.s:17-19`, `29` y `38-40`: EE y E0E a 0 en los dos caminos. Las tablas se escriben en LE y la MMU las lee en LE. `dtb.rs:72-76` y `base/mod.rs:44` leen el FDT con `from_be`. `nucleos.rs:29` escribe la tabla de espera en LE. Las copias de usuario son de byte en byte (`copia.s:16` y `27`), así que E0E no les afecta. No queda ninguna vuelta de bytes suelta. |
| B. Todo el `unsafe` en `base` | A medias | La forma está bien. Solo `base` y los módulos de pruebas tienen `unsafe` (`main.rs:13`, `17-51`). He comprobado que `deny(unsafe_code)` también para `global_asm!`. Pero varias funciones públicas de `base` no son sanas con cualquier argumento o en cualquier orden. Ver "Funciones de base que no son sanas". Además, `deny` se puede anular con un `#[allow(unsafe_code)]` en cualquier módulo nuevo. Hace falta una comprobación en `make` o en la revisión. |
| E. Sin expulsión con un cerrojo tomado | Bien en los cerrojos. A medias en el trabajo acotado | `cerrojo.rs:34-46` y `67-72`: todo cerrojo enmascara y repone. No he encontrado ninguna suelta en desorden salvo la de `soltar`, que está documentada (ver "Revisado y correcto"). `Canal::leer` (`objetos.rs:172-194`) cumple la regla. Los topes nuevos están: 4 MB en `memoria_crear` y 256 bytes en `escribir`. Pero siguen sin tope: 22 ms con las IRQ enmascaradas por cada `escribir` (provisional, anotado); soltar un proceso entero en `recoger()` al principio de cada llamada (`llamadas.rs:96`, anotado para la fase 3); y `Paginas::reservar`, que recorre la memoria entera página a página (ver M2, no anotado). |
| Pruebas fuera de la imagen | Bien | `main.rs:22-51` y `89-90`, `llamadas.rs:110-111`. He mirado `target/kernel8.img`: no tiene ninguna cadena de las pruebas. Siguen en EL1 dentro de la imagen de pruebas. Pasarlas a programas de usuario espera a la fase 3, como dice la bitácora. |
| Mapa por páginas, W^X y guardas | Bien | `mapa.rs:62-70` y `106-142`. El código es solo lectura y ejecutable. `.rodata` es solo lectura. Lo demás no se ejecuta. UXN en todo. La imagen está en el mapa lineal una sola vez: no hay alias escribible del código. Las pilas de los hilos están bien hechas (ver "Revisado y correcto"). Hay dos detalles bajos: la pila de emergencia (B1) y el operando de `tlbi` (B2). |
| PAN por software | A medias | Las llamadas, las excepciones y el cambio de hilo están bien. `excepciones.rs:51-61`, `usuario.rs:25-37` y `planificador.rs:413-415`. Pero el único camino que entra en EL0 por primera vez, `asignar_espacio` seguido de `entrar_el0`, no es atómico. C34 lo rompió. Ver M1. |
| Quitar la proyección identidad | Bien | `nucleos.rs:40-44` y `mapa.rs:233-241`. Las tablas del arranque se ponen a cero y se vacía la TLB en todos los núcleos, pero solo si llegan los tres núcleos secundarios. Eso está documentado. TTBR0 ya apunta a la tabla vacía en todos los núcleos antes de borrarlas (`planificador.rs:158-159`). |

## Funciones de base que no son sanas

Son funciones o tipos públicos de `base` que el código seguro de fuera puede usar para provocar comportamiento
indefinido o corromper memoria. Ninguno se puede alcanzar hoy desde EL0. La gravedad es para la decisión B.

### Media

**`excepciones::Marco`, campos públicos `elr` y `spsr`** (`excepciones.rs:9-16`). `llamadas::atender` recibe
`&mut Marco` (`excepciones.rs:92`). Desde código seguro se puede poner `marco.spsr = 0x5` y `marco.elr` a cualquier
dirección. El `eret` de `vectores.s:120` baja entonces a EL1h, no a EL0, en un PC elegido, y con los registros que
puso el proceso. Eso equivale a ejecutar código arbitrario en el kernel. Hoy `llamadas.rs` solo escribe `x[0]` y `x[1]`.
Arreglo: darle a `llamadas` una vista con solo los registros, o hacer privados `elr`, `spsr` y `sp_el0`. Comprobado.

**`planificador::tic()`** (`planificador.rs:442-444`) y **`planificador::bloquear(soltar)`** (`456-460`), junto con
**`cerrojo::restaurar_irq`** (`cerrojo.rs:83-85`) y **`cpu::habilitar_irq`** (`cpu.rs:32`). Todo el planificador
depende de que las IRQ sigan enmascaradas entre que se suelta el cerrojo del núcleo y `cambiar_contexto`
(`planificador.rs:425-434`). En ese momento `nucleo.actual` ya es el hilo nuevo, pero los registros todavía son los
del viejo. Hay dos formas de abrir las IRQ ahí desde código seguro:

- llamar a `tic()` con las IRQ abiertas: la guarda repone "abiertas" al soltarse;
- llamar a `bloquear(|| habilitar_irq())`, o pasar en `soltar` una guarda tomada con las IRQ abiertas.

Si entra un tic en ese hueco, `cambiar_contexto` guarda los registros del hilo viejo en el `Contexto` del hilo nuevo.
Dos hilos acaban corriendo sobre la misma pila. La regla está escrita en `cerrojo.rs:7-10` y en
`planificador.rs:283`, pero nada la hace cumplir. Arreglo: que `tic` sea `pub(super)`, y que `planificar_con`
enmascare por su cuenta antes de soltar el cerrojo y antes de `cambiar_contexto`. Comprobado.

**`espacio::Espacio::asid`, campo público** (`espacio.rs:65`). Con `e.asid = <ASID de otro espacio vivo>`, dos
procesos comparten ASID. Entonces se ven las traducciones del otro en la TLB y se lee o escribe su memoria. Y el `Drop`
(`76-77`) devuelve un ASID que no es suyo. Con `asid >= 65536`, `asid_devolver` cae con un índice fuera de rango. Nadie
fuera de `espacio.rs` lo usa: basta con quitarle el `pub`. Comprobado.

**`mapa::construir()`** (`mapa.rs:106-142`), si se llama dos veces. La segunda vez cambia `L2_PILAS` (`139`), pero
TTBR1 sigue en el mapa viejo. Al soltar una pila hecha antes, `Pila::drop` lee la nueva L2, encuentra 0
(`mapa.rs:208`) y escribe ceros en la página física 0 (`211`). Además, libera páginas que siguen proyectadas en el mapa
activo, y eso es usar memoria ya liberada. Hacerla `pub(crate)` no basta. Necesita una marca de "ya hecho". Comprobado.

**`memoria::Paginas::iniciar`** (`memoria.rs:102-155`), por `paginas().iniciar(...)` si se llama dos veces. Marca libre
todo lo que dice el nodo `memory` (`106`) y solo vuelve a reservar el kernel y el árbol. Todas las páginas en uso
(montón, tablas, pilas, procesos) quedan como libres y se volverán a dar. Debería ser `unsafe fn` o hacerse una sola
vez. Comprobado.

### Baja

**`planificador::terminar_actual()`** (`planificador.rs:546-573`) llamado desde el hilo ocioso, por ejemplo desde
`kmain` antes de `ocioso()`. El ocioso pasa a `Muerto` y entra en `muertos` (`407-411`), porque ahí no se comprueba
`actual != ocioso`. Luego `recoger` libera su `Box`. Pero `nucleo.ocioso` sigue apuntando a él (`376`), y eso es usar
memoria liberada la próxima vez que no haya nada que hacer. Comprobado.

**`planificador::HiloRef`, `yo()` y `HiloRef::despertar`** (`planificador.rs:489-508`). Si el hilo muere, el puntero
queda colgando. Ya está anotado en la bitácora para la fase 3. Hoy no lo he visto alcanzable: `Lado::esperando` siempre
se vacía con `take()` (`objetos.rs:163` y `205`), y el repartidor no muere nunca. No es peor de lo que dice.

**`mmio::RegLe32::new`** (`mmio.rs:15-18`) y **`Espacio::proyectar_dispositivo`** (`espacio.rs:245-257`). Solo
comprueban que la dirección esté en la ventana `0xFC00_0000..4 GB`. Esa ventana incluye los motores de DMA. Programarlos
escribe en cualquier sitio de la memoria. También incluye el GIC y el bloque local de ARM. No son sanas en sentido
estricto. Es la decisión D (sin IOMMU), ya conocida. Lo apunto para que la lista esté completa. Una lista de tramos
permitidos al dar la llave de registros lo reduciría.

**`mapa::activar()`** (`mapa.rs:223-229`) después de `olvidar_arranque`. `cambiar_ttbr1` salta a la proyección
identidad, que ya está a cero, y el núcleo cae sin mensaje. **`nucleos::despertar()`** (`nucleos.rs:24-46`) antes de
`mapa::construir()`: los secundarios no cambian de mapa (`activar` vuelve sin hacer nada si `L1 == 0`), y luego
`olvidar_arranque` borra las tablas que están usando todos. Las dos dependen del orden de llamada. Las dos dan un
cuelgue, no corrupción. Comprobado.

**`planificador::iniciar_nucleo()`** (`planificador.rs:138-161`) llamado dos veces: sustituye el `Nucleo` y pierde los
hilos listos y los muertos. Da cuelgue y fuga, no corrupción.

**`planificador::crear` y `crear_plazo`** (`planificador.rs:279-281`, `227`): un núcleo fuera de rango da pánico
(`NUCLEOS[nucleo]`), y quedarse sin pilas, también (`191`). Es pánico, no comportamiento indefinido, pero las dos son
seguras y públicas. Cuando un proceso pueda crear hilos, la segunda tendrá que devolver un error.

**`cerrojo::Guarda`** (`cerrojo.rs:49-52`). Por sus campos, Rust la hace `Send` y `Sync` con solo `T: Send`. Con un
`T` que no es `Sync`, como `Cell`, un `&Guarda` compartido entre núcleos da una carrera de datos. Para eso hace falta
un `Box::leak` de la guarda y un `static`, que es rebuscado. `std` declara `MutexGuard` como `!Send` y `Sync` solo si
`T: Sync`. Comprobado.

**`Espacio::pagina_nueva(.., Permiso::Dispositivo, ..)`** (`espacio.rs:262-286`) proyecta RAM normal como memoria de
dispositivo. Eso da atributos distintos para la misma página: el mapa lineal la ve normal y con caché. Solo afecta a
esa página. Muy baja.

El resto de lo público de `base` sí es sano: `cpu::*` (salvo `habilitar_irq`, ver arriba), `Cerrojo::tomar`,
`get_mut`, `enmascarar_irq`, `Espacio::nuevo`, `ttbr0`, `proyectar_memoria`, `Memoria`, `Paginas::pagina`,
`pagina_usuario`, `reservar` y `libres`, `mapa::pila_nueva` y `Pila`, `entrada`, `traducir`, `usuario::leer` y
`escribir`, `fallo_de_copia`, `arbol_de_dispositivos`, `mmu::*`, `nucleos::numero`, `proceso_actual*`, `ceder` y
`esperar_periodo`, `recoger`, `fp_atrapado`. `Paginas::liberar`, `crear_en` y `asignar_espacio` son `unsafe fn`, como
deben.

## Fallos nuevos

### Crítica y alta

Ninguno.

### Media

#### M1. La entrada en EL0 de un hilo nuevo puede hacerse con la mitad baja vacía

`pruebas.rs:210-214` y `528-533` (el patrón), `planificador.rs:591-597` (`asignar_espacio`),
`planificador.rs:535-538` (`hilo_estreno` abre las IRQ), `contexto.s:44-45` y `planificador.rs:413-415`.

Antes de C34, el cambio de hilo volvía a poner el TTBR0 del hilo que entraba. Desde C34 pone siempre la tabla vacía.
Solo la reponen `excepcion()` al volver a EL0 y las copias. `entrar_el0` no la repone.

Escenario:

1. Un hilo nuevo empieza en `hilo_entrada`. `hilo_estreno` abre las IRQ.
2. `arrancar_elf` llama a `asignar_espacio`. TTBR0 pasa a ser el del proceso. Las IRQ siguen abiertas.
3. Entra el tic antes del `msr daifset, #3` de `entrar_el0`, y el planificador cambia de hilo. TTBR0 pasa a la tabla
   vacía (`planificador.rs:415`).
4. Más tarde vuelve este hilo. Sale de la IRQ por el camino de EL1, así que `excepcion()` no toca TTBR0
   (`excepciones.rs:59`).
5. `entrar_el0` hace `eret` con la tabla vacía. La primera instrucción da un fallo de traducción en EL0, y el kernel
   termina el proceso recién nacido (`excepciones.rs:118-124`).

Hay un efecto más pequeño: si en el paso 3 la IRQ no cambia de hilo, todo su tratamiento corre con el TTBR0 del
proceso. Eso es justo lo que el PAN por software quiere evitar.

Gravedad media: no da acceso a nada, pero mata procesos al azar en el único camino de entrada a EL0, y el programa de
inicio lo heredará. La ventana es de pocas instrucciones. Comprobado leyendo. No lo he provocado.

Arreglo: que `entrar_el0` reciba el TTBR0 y lo cargue después de enmascarar. O que `asignar_espacio` solo lo apunte en
el hilo, y una función de `base` enmascare, lo cargue y baje a EL0.

#### M2. `Paginas::reservar` recorre toda la memoria con dos cerrojos y las IRQ enmascaradas (anterior a la fase 2, no anotado)

`memoria.rs:162-184`, llamada desde `monton.rs:61` para todo lo que pasa de 2 KB.

No es de estos commits, pero choca con el "trabajo acotado" de la decisión E y no está en la bitácora. `reservar` baja
página a página desde el tope, sin la pista que sí usa `pagina()`. Con la memoria alta ocupada, cada búsqueda recorre
todas las páginas. Son 245.000 en QEMU y hasta 1 M con 4 GB. Lo hace con los cerrojos del montón y de las páginas
tomados y las IRQ enmascaradas.

Escenario desde EL0:

1. El proceso llena su memoria con `memoria_crear` de 4 MB.
2. Sigue llamando a `memoria_crear`. Cada llamada pide un `Vec` de 8 KB (`memoria.rs:268`) antes de mirar si quedan
   páginas.
3. Cada llamada barre el mapa entero. Con tres procesos así, en tres núcleos, el cerrojo de páginas está casi siempre
   tomado.

Calculo unos milisegundos por llamada en la Pi 4. No está medido. Plausible.

### Baja

#### B1. La pila de emergencia no cumple la regla del bit 14, y su pánico puede colgarse

`vectores.s:52-67`, `excepciones.rs:45-47`.

La pila de emergencia es de 4 KB por núcleo en `.bss`, sin alinear a la ranura. En el ELF que hay ahora en `target/`
va de `0x…1ca000` a `0x…1ce000`. Las de los núcleos 2 y 3 tienen el bit 14 a 0. Una excepción dentro de
`pila_desbordada` en esos núcleos vuelve a `desbordada`, que repone la misma pila y empieza otra vez. Además,
`pila_desbordada` llama a `actual_id()`, que toma el cerrojo del núcleo, y a `kprintln!`, que toma el de la serie. Si
el desbordamiento ocurrió con uno de esos cerrojos tomado, el núcleo se queda girando con las IRQ enmascaradas y no sale
ningún mensaje. No es explotable: el kernel ya iba a pararse. Plausible.

#### B2. El operando de `tlbi vaae1is` lleva bits que deberían ser 0

`mapa.rs:212`. Se pasa `va >> 12` de una dirección alta, así que los bits 44 a 51 quedan a 1. En el A72 y el A76 son
RES0 y se ignoran. En una CPU con FEAT_TTL (ARMv8.4), los bits 47:44 son la pista de nivel, y "1111" dice "granulo de
64 KB, nivel 3", que no es el caso. La invalidación podría no hacerse, y la pila liberada seguiría visible por una
entrada vieja de la TLB. Linux enmascara a 44 bits. Hoy no afecta a la Pi 4 ni a la Pi 5. Plausible.

#### B3. Volver a EL0 con `ttbr0_actual() == 0` cargaría la página física 0 como tabla

`excepciones.rs:59-60`. Si un hilo sin espacio volviera a EL0, `activar(0)` pondría en TTBR0 la página física 0, que
tiene el código del firmware. Hoy no es alcanzable: los hilos del kernel no vuelven a EL0, y `terminar_actual` no
vuelve. Por robustez, con 0 debería ir la tabla vacía. Comprobado.

#### B4. Las pilas de los hilos ya no salen a cero

`mapa.rs:186-194`. Antes eran `vec![0u8; PILA]`. Ahora son páginas del repartidor sin limpiar, y pueden traer datos
de un proceso muerto o de otra pila. Rust no lee pila sin inicializar, y las copias hacia el usuario salen de búferes
inicializados. Solo importa si algún día se copia al usuario una estructura con relleno. Muy baja.

## Revisado y correcto

**`cambiar_ttbr1`** (`arranque.s:143-176`). Corre con todo DAIF enmascarado. Lee su literal antes de tocar TTBR1.
Salta a su dirección física por la identidad, que en el arranque es ejecutable en EL1. Pone TTBR1 vacío, vacía la TLB
local, pone el nuevo, vacía otra vez, vuelve a la mitad alta y repone TTBR0. No toca la pila ni la memoria entre medias.
El `dsb ish` de `mapa.rs:140` hace visibles las tablas a los otros núcleos antes de que las usen. Las entradas globales
de la identidad se van con los `tlbi vmalle1` locales y con el `vmalle1is` de `olvidar_arranque`. Bien.

**La comprobación del bit 14** (`vectores.s:14-25`). La aritmética es correcta: x0 y SP vuelven a su valor sin tocar
memoria. Restar los 288 bytes antes de mirar cubre el caso de un SP justo encima de la guarda y el del tope de la
ranura, cuyo bit 14 es 0. Las pilas de arranque (`__pilas_inicio` alineado a 32 KB, `enlace.ld:26-29`) y las del GB
256 cumplen la regla. Un marco de más de 16 KB no puede saltarse la guarda: el objetivo usa sondas de pila en línea
(`stack-probes: inline`), y el único marco grande del binario es de 4 KB.

**SP_EL0.** El kernel corre siempre en EL1h: `SPSR_EL2 = 0x3c5` (`arranque.s:32`). Las entradas 0 a 3 no se alcanzan.
El SP_EL0 del proceso se guarda y se repone en el marco (`vectores.s:92-93` y `101-102`) y el kernel no lo usa nunca.
Hay un detalle anterior a la fase 2: si el firmware entrara directamente en EL1 (`arranque.s:38-42`), no se fuerza
`SPSel = 1`. La Pi 4 y QEMU entran en EL2.

**`recoger_serror`** (`copia.s:44-51`). Solo abre A, no I. El ELR de un SError tomado ahí cae en `[serror_inicio,
serror_fin]`. Se llama antes de poner la tabla vacía, así que el SError se le cobra al proceso que acaba de entrar y
`terminar_actual` lo libera todo con TTBR0 ya vacío. Es correcto que `excepcion` anidada no vuelva.

**W^X y alias.** `enlace.ld` pone cada sección en página propia. El ELF compilado tiene `.text` R E, `.rodata` R y
`.data`, `.bss` y `.tablas` RW, sin secciones huérfanas. Las páginas de la imagen están reservadas en el repartidor, así
que no llegan ni al montón ni a un proceso. Las únicas direcciones del usuario que se pueden proyectar vienen de
`Memoria` (páginas del repartidor) o de la ventana de periféricos. No hay ningún alias escribible del código. La tabla
vacía es un `static` inmutable.

**Pilas del GB 256** (`mapa.rs:144-220`). La ranura se elige y se marca bajo el mismo cerrojo. El nivel 3 se crea una
sola vez y a cero. Si falla a medias, devuelve las páginas. Al soltar, borra las entradas y hace `tlbi vaae1is` con
`dsb ish` antes de liberar las páginas y la ranura: no queda traducción vieja hacia páginas reutilizadas (con la
salvedad de B2). Las entradas son globales, y la invalidación por VA para todos los ASID es la correcta. Solo libera
una pila `recoger`, en el mismo núcleo, cuando el hilo muerto ya terminó su cambio de contexto (IRQ enmascaradas entre
`planificador.rs:410` y `433`).

**Guarda de las pilas de arranque** (`mapa.rs:73-86`, `enlace.ld:26-29`). La mitad baja de cada ranura no se
proyecta. El núcleo n usa la ranura 3−n, en su mitad alta. Las pilas pasan de 64 a 16 KB. Las pruebas pasan, según la
bitácora, y no he visto caminos profundos.

**PAN en llamadas y excepciones.** Toda entrada desde EL0 pone la tabla vacía (`excepciones.rs:51-55`). Toda vuelta a
EL0 por `excepcion` pone la del hilo que vuelve, que puede ser otro (`59-61`). `con_espacio` pone la del proceso solo
alrededor de la copia y la quita al acabar (`usuario.rs:25-37`). Las llamadas corren con las IRQ enmascaradas, así que
no hay cambio de hilo a mitad de una copia. Un fallo dentro de la copia vuelve por `copia_fallo` sin pasar por la
vuelta a EL0. Con ASID 0 en la tabla vacía, las entradas de la TLB del proceso no valen dentro del kernel.
`terminar_actual` pone la tabla vacía antes de soltar el espacio. Fuera del camino de M1, no he encontrado ningún sitio
donde el kernel corra con la mitad baja de un proceso.

**Cerrojos y planificador.** Las guardas se sueltan en orden inverso en todo el kernel normal: `paginas()` y los
cerrojos de `espacio`, de `llaves` y de `ASID` se toman y sueltan anidados. La única excepción es `soltar` dentro de
`planificar_con`, y es correcta porque `Canal::leer` enmascara antes de tomar el cerrojo del canal. Un hilo que vuelve
de `cambiar_contexto` repone su propio DAIF (`bloquear`, `ceder`, `esperar_periodo`, la salida de la IRQ con su SPSR),
y uno nuevo abre las IRQ en `hilo_estreno`. No hay ciclos de cerrojos. Los órdenes son estos:
- canal → núcleo, en `bloquear`;
- núcleo → montón → páginas, en `crear_en` y `fp_atrapado`;
- espacio → montón y páginas;
- ranuras → páginas.

`despertar` siempre se llama con el cerrojo del canal y el de `REPARTIDOR` ya soltados (`objetos.rs:165-167`,
`interrupciones.rs:103-106`). La regla de "no expulsar con un cerrojo tomado" se cumple también en el repartidor: sus
cerrojos ya enmascaran.

**Despertar sin perder avisos.** El hilo consta como bloqueado antes de soltar el cerrojo del canal. Otro núcleo que
lo despierte lo mete en la cola, pero solo su núcleo lo puede correr, y ese núcleo no abre las IRQ hasta terminar de
guardar el contexto. Bien.

**`usuario::leer` y `escribir`.** Comprueban el límite de 39 bits con suma sin desbordamiento. Sin espacio devuelven
falso. Las copias son de byte en byte, con LDTRB y STTRB, así que E0E no les afecta. Los fallos vuelven por
`fallo_de_copia`, y solo se reconocen si vienen de `[copia_inicio, copia_fin)` y de EL1.

**`arbol_de_dispositivos`** (`base/mod.rs:38-49`). Comprueba la alineación, que esté fuera de la ventana de periféricos,
la marca y un tamaño de como mucho 2 MB. El tramo está reservado en el repartidor (`memoria.rs:122`) con ese mismo
tamaño, así que el `&'static [u8]` no se pisa. `dtb.rs` ya no hace accesos sin límite.

**Pánicos desde EL0.** No he encontrado pánicos nuevos alcanzables desde EL0. Los `expect` nuevos (`mapa.rs:89`,
`planificador.rs:191`) solo se alcanzan al arrancar o al crear hilos, que hoy solo hace el kernel. No hay recursión
nueva con la pila de 16 KB.
