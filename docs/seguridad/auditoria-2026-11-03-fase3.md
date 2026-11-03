# Auditoría de cierre de la fase 3 de S16, 2026-11-03

Hecha por un agente con lectura independiente. No se ha ejecutado QEMU ni se ha cambiado código. Mira los commits C38
a C43 (`7e2dc75..8abf30b`) y lo que tocan de `kernel/`. Los números de línea son de `8abf30b`. Este fichero no se
actualiza: es la foto de ese día.

Modelo de amenaza: un programa hostil en EL0, dentro de un grupo con cupo, con cualquier llamada, valor e instrucción,
y con las llaves que le den. Varios procesos hostiles que colaboran. Un controlador con llave de registros o de
interrupción.

"Comprobado" quiere decir comprobado leyendo el código. "Plausible" quiere decir que el camino existe, pero depende de
una carrera, del tamaño de los marcos de pila o del hardware, y no lo he seguido hasta el final.

Igual que en la fase 2: la imagen normal no crea ningún proceso (`main.rs:95`). Lo que sigue sobre EL0 se ve hoy solo
en la imagen de pruebas, y será el camino del programa de inicio cuando llegue. Las gravedades son para ese día.

## Estado de la fase 3

| Parte | Estado | Dónde y por qué |
|---|---|---|
| F. Cupos por grupo | A medias | Se cobran bien las páginas de memoria, el espacio, el proceso y el canal (`cupo.rs:70-89`, `espacio.rs:131-168`, `memoria.rs:323-332`, `procesos.rs:145`, `objetos.rs:256-258`). Pero las revocables no se cobran y se pueden encadenar sin fin (C1). Los ciclos de canales dejan su cargo cobrado para siempre, también tras matar el grupo (M1). Matar un grupo no alcanza a un proceso que se está creando (M2). Y crear un proceso puede tumbar el kernel en vez de dar `E_MEMORIA` (M4). |
| G. Duplicar con menos derechos | Bien | `llamadas.rs:508-524`. Los derechos nuevos son `l.derechos & derechos`. Una llave revocada no se duplica (`513`). |
| G. Marca del que escribe | Bien | `llamadas.rs:516-519`: una llave marcada no se vuelve a marcar con otra. `228` pone la marca de la llave en el mensaje y `297` la devuelve. Revocable y duplicado conservan la marca. Ver B6 para el valor 0. |
| G. Revocar | Mal | La resolución está bien: todas las llamadas pasan por `TablaLlaves::ver` (`objetos.rs:394-401`), y duplicar, mandar y pasar a un hijo mueven el envoltorio, no el objeto. No he encontrado forma de sacar el objeto de detrás. Pero una revocable puede envolver a otra sin límite, sin cobrarse, y soltar la cadena es recursivo: tumba el kernel (C1). Además, una lectura bloqueada y una interrupción asociada sobreviven a la revocación (B1, B2). |
| G. Sin ciclos | A medias | `objetos.rs:86-92` rechaza un lado del mismo canal, también tras una revocable. Pero no mira dentro de un revocador, y con él se hace el ciclo de un solo canal (M1). Los de varios canales siguen, como dice la bitácora, pero su efecto es peor de lo que dice (M1). |
| G. Destrucción en bucle | Mal | `soltar` (`objetos.rs:207-240`) es un bucle para puntas, revocables e interrupciones. Pero un `Revocador` o un `Grupo` caen en `_ => {}` (`236`) y se sueltan por la vía normal de Rust, que es recursiva. Fuera de `soltar` no hay `Drop` propio para `Revocable` ni para `Cupo`. Ver C1 y A1. |
| G. Cerrar al morir | Bien | `objetos.rs:186-202`: el último `Arc<Punta>` cierra el lado y despierta al otro. Vale para `cerrar`, para la muerte del proceso (se suelta su tabla en `planificador.rs:611`) y para una llave perdida en un mensaje. Salvo los ciclos de M1. |
| Crear procesos | A medias | `llamadas.rs:397-462` y `procesos.rs:144-176` están bien: el ELF se copia al kernel antes de leerlo, el tamaño se comprueba y las llaves vuelven a su sitio si algo falla. Pero `crear_hilo` acaba en pánico si no hay pila o montón (M4). |
| Matar procesos | A medias | El marcado, la comprobación al volver a EL0 y la del planificador están bien (ver "Revisado y correcto"). Un hilo marcado no escapa. Pero un proceso que se está creando sí (M2). Y quien ya sacó un `HiloRef` de su sitio puede despertarlo después de que el hilo muera y se libere (A2). |
| Esperar procesos | A medias | `llamadas.rs:475-502` está bien: varios esperan a la vez y cada uno se quita al despertar por muerte. El fallo está del lado del que termina: despierta a la lista fuera del cerrojo (A2). |
| C38. Fuera del alcance del DMA | Bien en QEMU. A medias en la placa | Las zonas, el mapa y `mmu::fisica` están bien (ver "Revisado y correcto"). Pero la copia de la imagen solo se limpia hasta el punto de unificación (`arranque.s:210`), y los núcleos 1 a 3 arrancan de ella con la MMU apagada. En la Pi 4 pueden leer memoria vieja (M5). QEMU no simula cachés. |

## Fallos nuevos

### Crítica

**C1. Una cadena de revocables tumba el kernel.** Comprobado leyendo. El tamaño exacto de la cadena depende de los
marcos de pila, que no he medido.

- `llamadas.rs:534` y `538`: `llave_revocable` toma la llave cruda con `ver_crudo` y la envuelve. Si ya era revocable,
  el intermediario nuevo contiene otro intermediario. No hay límite de profundidad.
- `objetos.rs:46-61`: `Revocable` no tiene `Drop` propio. Soltar el de fuera suelta el de dentro, y así hasta el
  fondo, por recursión del compilador.
- Nada de esto se cobra al cupo: un `Arc<Revocable>` son unos 64 bytes del montón.

Escenario, desde cualquier grupo y sin llaves especiales:

1. `memoria_crear(4096)` da la llave `h`.
2. Repetir unas miles de veces: `(c, r) = llave_revocable(h)`; `cerrar(h)`; `cerrar(r)`; `h = c`. La tabla no crece.
   Cada intermediario queda vivo solo dentro del siguiente.
3. `cerrar(h)`, `revocar` o `salir`. La cadena se suelta por recursión en la pila del kernel, de 16 KB. Unos cientos
   de niveles bastan. La pila llega a la guarda y `excepciones.rs:68-70` hace `panic!`. El kernel se para.

Variantes con el mismo origen:

- Con la cadena hecha de revocadores (`llave_revocable` sobre el revocador anterior), `ver` no recorre la cadena,
  cada paso cuesta lo mismo, y la cadena puede crecer hasta agotar el montón. El montón saca páginas de toda la zona
  normal, también de la reserva (`monton.rs:42`). Al fallar, `Arc::new` cae en pánico. Esto desmiente
  `docs/referencia/kernel.md:301-302`, que dice que un proceso con cupo no puede agotar el montón.
- Con la cadena de revocables simple, cada `ver` recorre la cadena entera con el cerrojo de la tabla tomado y las IRQ
  enmascaradas (`objetos.rs:396-399`). Es trabajo sin tope (decisión E).
- Un `Revocador` dentro de un mensaje cae en `_ => {}` de `soltar` (`objetos.rs:236`). Si su intermediario envuelve
  una punta de otro canal, el `Drop` de esa punta vuelve a llamar a `soltar`. Con una cadena de canales, cada uno con
  el revocador del siguiente en su cola, la recursión vuelve: unos miles de canales caben en 64 MB.

Arreglo propuesto: una lista de trabajo para soltar objetos, no solo mensajes, y un `Drop` de `Revocable` y de
`Revocador` que la use. Además, limitar la profundidad de los intermediarios (o resolver al crear, de modo que una
revocable apunte siempre a un objeto y no a otro intermediario) y cobrar cada intermediario al cupo.

### Alta

**A1. Soltar una cadena de grupos anidados también es recursivo.** Plausible: el camino está comprobado, el número de
niveles que hacen falta depende de los marcos de pila.

- `cupo.rs:30`: cada subgrupo guarda un `Cargo` sobre su padre, y `cupo.rs:36-45` suelta ese `Arc<Cupo>` al
  soltarse. Si el subgrupo era lo último que tenía vivo al padre, el padre se suelta dentro, y así hacia arriba.

Escenario, desde un grupo de 64 MB:

1. P0 crea un subgrupo G1 y lanza en él a P1, con una copia de su ELF.
2. P1 hace lo mismo con G2, y así hasta unos cientos de niveles. Cada nivel cuesta unos 64 KB más su espacio: caben
   varios cientos en 64 MB.
3. Cada Pi cierra su llave de grupo y sale. Cada Gi queda vivo solo por el cargo de Gi+1.
4. Cuando sale el último, su grupo se suelta y arrastra a todos los de arriba por recursión, en la pila del hilo que
   muere (`planificador.rs:611`). Pánico como en C1.

Arreglo: soltar el padre fuera, con una lista (por ejemplo, que `Cargo::drop` devuelva el `Arc` a una cola), o
limitar la profundidad de los subgrupos.

**A2. Un hilo se puede despertar después de liberado.** Plausible: es una carrera. El camino está comprobado leyendo.

C39 cerró el caso del hilo que muere con su `HiloRef` todavía guardado: se quita él al despertar. No cerró el caso en
que otro ya lo ha sacado de su sitio y aún no lo ha despertado. En tres sitios se saca con el cerrojo y se despierta
después de soltarlo:

- `planificador.rs:598-610`: `terminar_actual` saca la lista de los que esperan a ese proceso y los despierta uno a
  uno fuera del cerrojo del `Control`.
- `objetos.rs:284-288`: `Canal::escribir`.
- `objetos.rs:337-341`: `Canal::cerrar_y_vaciar`.

Escenario con procesos que colaboran:

1. X es un proceso. W1 a W50 tienen cada uno una copia de la llave de X con derecho de leer, y esperan en
   `proceso_esperar(X)`.
2. X sale. En el mismo momento, K, que tiene las llaves de los W, los mata.
3. El núcleo de X saca la lista y empieza a despertar. Cada despertar toma un cerrojo de núcleo y puede mandar una SGI.
4. W50, despertado por la muerte (`planificador.rs:700-701`), corre en su núcleo, ve el fin de X, vuelve, muere al
   bajar a EL0 y `recoger` libera su `Hilo` en la siguiente llamada de ese núcleo (`planificador.rs:720-726`).
5. X llega a W50 y llama a `despertar` sobre memoria liberada (`planificador.rs:497`). Si la memoria se ha reusado,
   `nucleo` puede valer cualquier cosa: con 4 o más, pánico por índice; si no, y `estado` parece "bloqueado", se mete
   en la cola un puntero falso (`planificador.rs:508-512`) y el planificador cambia a un contexto que no es de ningún
   hilo.

En el repartidor la ventana es mayor: `entregar` corre con las IRQ abiertas (`interrupciones.rs:134-141`), así que un
tic puede caer entre soltar el cerrojo del canal y despertar.

Arreglo: despertar con el cerrojo tomado. El orden lo permite: el del núcleo es el último. O hilos con cuenta de
referencias. De paso, `HiloRef::despertar` (`planificador.rs:531-537`) es una función pública y segura que usa un
puntero crudo; según la regla 6 de `AGENTS.md` debería ser `unsafe fn` o no ser pública.

### Media

**M1. Ciclos: el revocador se salta la comprobación, y ningún ciclo se libera nunca.** Comprobado leyendo.

`forma_ciclo` (`objetos.rs:86-92`) mira la llave tras `ver`, que resuelve revocables pero no revocadores.

1. `canal_crear` da `a` (lado 0) y `b` (lado 1).
2. `(rb, rev) = llave_revocable(b)`; `cerrar(b)`. La punta del lado 1 queda solo dentro del intermediario.
3. `canal_escribir(a, ..., [rev])`. El mensaje va a la cola del lado 1. `forma_ciclo` dice que no.
4. `cerrar(rb)`; `cerrar(a)`. Cerrar el lado 0 solo vacía la cola del lado 0. La punta del lado 1 la tiene el
   intermediario, que lo tiene el mensaje, que está en la cola del lado 1. No se libera nunca.

La bitácora (C40) dice que los ciclos de varios canales "los acotan los límites (y los cupos de C42)". Es verdad para
cada grupo, pero se queda corto:

- El cargo del canal está dentro del ciclo, así que el grupo nunca vuelve a 0 y su `Cupo` nunca se suelta. Su padre
  pierde ese presupuesto para siempre. `grupo_matar` no lo libera: mata procesos, no canales.
- Lo que se pierde de verdad es montón del kernel: unos 20 KB por canal con las colas llenas, sacados también de la
  reserva. Si un supervisor del grupo raíz relanza al programa hostil, cada vez se pierde otro tanto, sin límite.

Arreglo: que `forma_ciclo` resuelva también revocadores. Para los de varios canales, que el grupo lleve la lista de
sus canales y al matarlo se cierren los dos lados de cada uno, o comprobar la alcanzabilidad al mandar un extremo.

**M2. Matar un grupo no alcanza a un proceso que se está creando.** Plausible: es una carrera.

- `procesos.rs:153` apunta el `Control` en el grupo, y el hilo se crea después (`procesos.rs:174`, que acaba en
  `planificador.rs:234-236`).
- `Cupo::matar` (`cupo.rs:108-117`) toma una foto de los miembros. `matar_proceso` (`planificador.rs:690-706`) solo
  marca los hilos que ya están en la lista.
- Ni el `Cupo` ni el `Control` guardan que se les ha matado.

Escenario: P, en el grupo G, llama a `proceso_crear` en bucle. Un supervisor llama a `grupo_matar(G)`. Si la foto se
toma antes de `apuntar`, o entre `apuntar` y el alta del hilo, el hijo no se marca. P muere al volver a EL0. El hijo
corre en un grupo "matado" y puede repetir. Esto rompe "matar siempre funciona" (S2).

Arreglo: una marca de muerto en el `Cupo`, mirada en `apuntar` con el mismo cerrojo; y otra en el `Control`, mirada en
`crear_hilo` al apuntar el hilo, que lo cree ya marcado.

**M3. Dos lectores en el mismo lado: uno se queda colgado.** Comprobado leyendo.

- `objetos.rs:142`: cada lado guarda un solo hilo en espera. `objetos.rs:320` lo pisa sin mirar.
- Antes de C41 una llave de canal tenía un solo dueño. Con `llave_duplicar` (`llamadas.rs:508-524`), dos procesos
  pueden leer el mismo lado.

Escenario: un servidor da a dos clientes copias de un lado con derecho de leer, o hay varios trabajadores leyendo una
misma cola. T1 se bloquea. T2 se bloquea y pisa a T1. Llega un mensaje: despierta T2. T1 no despierta nunca, ni con
más mensajes ni cuando el otro lado se cierra (`objetos.rs:337` solo despierta al guardado). Solo sale si lo matan. Un
cliente hostil cuelga a otro con solo llamar a `canal_leer`.

Arreglo: una lista de hilos en espera por lado, como en `proceso_esperar`, o despertar a todos.

**M4. Crear un proceso puede acabar en pánico.** Plausible: depende de los presupuestos que se repartan.

- `planificador.rs:202`: `pila_nueva().expect("sin pilas del kernel")`. El comentario de `201` dice "Hoy solo crea
  hilos el kernel". Desde C39 no es así: `llamadas.rs:439` llega aquí.
- `pila_nueva` (`mapa.rs:213-246`) da `None` sin ranuras (32768) o sin páginas. Sus páginas salen de `pagina`, que sí
  toca la reserva.
- En el mismo camino, sin `try`: `Box::new` del hilo (`planificador.rs:218`) y del arranque (`676`), `Arc::new` del
  `Control` y del `Proceso` (`procesos.rs:152` y `164`) y el `push` de `planificador.rs:235`.

El cupo cuenta bytes, pero no los aparta: la suma de los presupuestos puede pasar de la memoria que hay, y el grupo
raíz no tiene límite. Escenario con grupos hostiles que colaboran: llenan la memoria de procesos hasta la reserva
(`pagina_usuario` para ahí); después crean procesos, cuyas pilas y estructuras salen de la reserva; al agotarla,
pánico.

Arreglo: que `crear_hilo` devuelva un error y `lanzar` lo convierta en `Error::SinMemoria`, con reservas falibles en
ese camino.

**M5. En la Pi 4, los núcleos 1 a 3 pueden arrancar de memoria vieja.** Plausible, solo en la placa.

- `arranque.s:204-207` copia la imagen con la caché encendida y `arranque.s:210` la limpia con `dc cvau`, es decir,
  hasta el punto de unificación. En el Cortex-A72 eso es la L2; la copia puede no haber llegado a la RAM.
- `nucleos.rs:30` da a los núcleos 1 a 3 la dirección de la copia. Entran en EL2 con la MMU y las cachés apagadas. Sus
  instrucciones y las constantes de `ldr x2, =...` (`arranque.s:29`, `48`, `50`) se leen sin caché, de la RAM.
- Si la línea sigue sucia en la L2, el núcleo ejecuta lo que hubiera antes en esa memoria, en EL2.

En QEMU no pasa porque no simula cachés. Arreglo: limpiar la copia hasta el punto de coherencia (`dc cvac`) en
`trasladar`, o arrancar los núcleos desde la imagen vieja, que sigue entera y reservada.

### Baja

**B1. Una lectura bloqueada sobrevive a la revocación.** Comprobado leyendo. `canal_leer` (`llamadas.rs:252-258`)
resuelve la llave y se bloquea con su propio `Arc<Punta>`. `revocar` (`objetos.rs:57-60`) solo suelta la referencia
del intermediario. Mientras el revocado siga bloqueado, el lado no se cierra y el siguiente mensaje le llega a él.
Después, sus llamadas dan `E_LLAVE`.

**B2. Una interrupción sigue escribiendo tras revocar la llave del canal.** Comprobado leyendo.
`interrupcion_asociar` (`llamadas.rs:352-366`) guarda el `Arc<Canal>` en la interrupción (`interrupciones.rs:56-57`),
fuera de la punta y del intermediario. Hace falta una llave de interrupción.

**B3. Se pierde el `Arranque` de un proceso que muere antes de correr.** Comprobado leyendo.
`arrancar_proceso` (`planificador.rs:664-669`) tiene el `Box<Arranque>` en una variable local. Si `bajar_a_el0` lo
termina (`planificador.rs:647-648`), `terminar_actual` no vuelve y el `Box` no se suelta. Son 16 bytes del montón sin
cobrar por cada hijo matado antes de correr. Crecen sin límite.

**B4. Memoria del kernel que no se cobra.** Comprobado leyendo. Aparte de las revocables de C1:

- el `Cupo` de cada subgrupo (`cupo.rs:62`);
- el `Control` de un proceso que ya acabó, mientras quede una llave suya (`procesos.rs:152`);
- la tabla de llaves crece con `try_reserve(1)` (`objetos.rs:375-378`) y puede quedarse con capacidad para 2048
  llaves de 40 bytes: unos 80 KB, más que los 64 KB de `COSTE_PROCESO`;
- `Canal::devolver` (`objetos.rs:327-329`) no mira `MAX_COLA`: si otro escribe entre leer y devolver, la cola pasa de 16.

Todo esto lo acotan las 1024 llaves por proceso, así que es un factor sobre el cupo, no una fuga.

**B5. El orden de cerrojos no es el que se dice.** Comprobado leyendo. No he encontrado un ciclo.

- `crear_hilo` toma el del núcleo y luego el del `Control` (`planificador.rs:210` y `235`). `matar_proceso` los toma
  al revés (`691` y `695`). Hoy no se cruzan porque el `Control` es nuevo y no tiene hilos.
- `llave_duplicar` y `llave_revocable` sueltan una copia resuelta con el cerrojo de la tabla tomado
  (`llamadas.rs:513` y `535`). Si otro revoca a la vez, esa copia puede ser la última de una punta: se cierra un canal
  dentro del cerrojo de la tabla.
- `proceso_crear` llama a `matar_proceso` con el cerrojo de la tabla tomado: la guarda de `llamadas.rs:442` vive hasta
  el final del `match`.

**B6. La marca 0 es a la vez "sin marca" y "del kernel".** Comprobado leyendo. Quien recibe una llave sin marca puede
ponerle la que quiera (`llamadas.rs:519`), también la de otro cliente. Y sus mensajes llegan con marca 0, como los de
una interrupción (`interrupciones.rs:152`). Es del diseño. Conviene decirlo en `docs/referencia/kernel.md`: un
servidor solo debe dar llaves marcadas.

**B7. Comentarios desfasados.** `planificador.rs:518-520` dice que un hilo bloqueado no puede morir. `planificador.rs:201`
dice que solo el kernel crea hilos. `objetos.rs:20-21` dice que los límites son provisionales hasta los cupos, que ya
están.

## Revisado y correcto

- **Un hilo marcado no escapa.** `matar_proceso` marca con el cerrojo del núcleo del hilo
  (`planificador.rs:694-699`). El planificador no bloquea a un hilo marcado y lo mira con ese mismo cerrojo
  (`planificador.rs:323`), así que no hay hueco entre marcar y bloquearse. Muere al volver a EL0
  (`excepciones.rs:82-86`) y antes de bajar por primera vez (`planificador.rs:647-648`). Un hilo expulsado en EL0
  vuelve por `excepcion`, y las llamadas corren con las IRQ enmascaradas, así que no hay otro sitio donde quedarse.
  Los únicos bloqueos desde EL0 son `Canal::leer` y `proceso_esperar`, y los dos miran la marca al despertar
  (`objetos.rs:311`, `llamadas.rs:492`).
- **La lista de hilos del `Control`.** Se cambia con su cerrojo (`planificador.rs:235` y `599-600`), y
  `matar_proceso` lo tiene mientras usa los punteros. Un hilo se quita antes de marcarse muerto. No queda ningún
  `HiloRef` suelto ahí.
- **`HiloRef` guardados al morir.** Cuando nadie los ha sacado, el hilo se quita él: `objetos.rs:308-310` y
  `llamadas.rs:486`. El fallo de A2 es solo el de quien ya lo sacó.
- **Puntas y canales.** Dos puntas del mismo canal soltadas a la vez en dos núcleos: las dos fallan `try_unwrap`, y la
  última referencia acaba en el `Drop` de `Canal`, que vacía y suelta. No se pierden mensajes que alguien pudiera
  leer: solo se sueltan los de un lado que ya nadie lee. Escribir a un lado cerrado da `E_CERRADO` y devuelve las
  llaves a su hueco (`llamadas.rs:229-243`). Las interrupciones dentro de mensajes se sueltan sin recursión.
- **Revocación sin escapes de objeto.** No hay forma de sacar el objeto de detrás de un intermediario:
  `llave_duplicar` y `llave_revocable` copian la llave cruda, `canal_escribir` y `sacar_llaves` la mueven, y todas las
  demás llamadas solo usan lo que da `ver` mientras dura la llamada. `revocar` suelta fuera del cerrojo
  (`objetos.rs:57-60`). Lo ya proyectado sigue proyectado, como dice la bitácora.
- **Matar sin llave.** `proceso_matar`, `grupo_matar` y `proceso_crear` con grupo piden el derecho de escribir en una
  llave del tipo correcto. Un proceso solo crea subgrupos del suyo y no puede salirse de su cupo.
- **Cuentas del cupo.** `cobrar_suelto` usa `checked_add` y CAS (`cupo.rs:76-85`). `Espacio` cobra y devuelve cada
  página por pares, también en los errores (`espacio.rs:131-168`, `206-209`, `299-322`) y en su `Drop` (`86`).
  `Memoria` devuelve las páginas tomadas si falla a medias. Un subgrupo devuelve su presupuesto al soltarse. `matar`
  no es recursivo y no tiene cerrojos tomados al matar.
- **`proceso_crear`.** El ELF se copia del objeto al kernel antes de cargarlo (`llamadas.rs:426-433`): cambiar el
  objeto después no afecta. Si la llave del hijo no cabe, el hijo muere. Si falla el lanzamiento, las llaves vuelven a
  sus números.
- **C38, zonas.** `piso` se calcula una vez y `libres_bajo` se lleva bien en `marcar` (`memoria.rs:76-93`). `pagina` y
  `reservar` no bajan del piso en la zona normal (`memoria.rs:208-232`, `254-277`). `pagina_usuario` cuenta la
  reserva solo sobre la zona alta (`memoria.rs:286`). El montón y las pilas salen de la zona alta.
- **C38, mapa y traslado.** La copia no tiene alias en el mapa lineal (`mapa.rs:150-151`). La imagen vieja sigue
  reservada y su hueco del mapa lineal apunta a la copia, así que no se ve. `mmu::fisica` traduce las direcciones de la
  imagen. Ningún código hace `virt(fisica(...))` de algo de la imagen, que caería en el hueco. Los valores que se
  calculan antes del traslado (tabla vacía, tablas del arranque) solo se usan durante el cambio de mapa, y la imagen
  vieja no cambia. `sec_alto` cambia de mapa antes de usar la pila. `trasladar` copia y cambia con las IRQ
  enmascaradas y sin otros núcleos en marcha.
- **C38, árbol de dispositivos.** Se copia al montón (`base/mod.rs:38-43`) y todo lo posterior lee la copia
  (`dtb.rs:55`). El original solo se usa en `kmain` durante el arranque, y sus páginas siguen reservadas.

## Qué no he mirado o no he podido comprobar

- No he ejecutado nada. Los tamaños de marco de C1 y A1 son estimados.
- Las pruebas de C43 (`pruebas.rs` y los programas de `usuario/`) solo por encima.
- El trabajo sin tope de soltar un proceso entero dentro de `terminar_actual`, con las IRQ enmascaradas, ya venía de la
  fase 2. Lo acota el cupo, pero sigue sin trocear.
