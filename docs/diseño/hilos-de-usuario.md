# Hilos de usuario

Encargo: varios hilos por proceso. Lo piden LKL (S12b: los hilos del kernel de Linux son hilos del proceso "linux")
y muchos programas (Dash, el puente, la caja Amiga con varias tareas en un proceso). Hoy cada proceso nace con un
hilo y no hay llamada para crear más. Bitácora: `docs/bitacoras/hilos.md`.

## Lo que ya hay

El kernel está hecho casi entero para varios hilos por proceso, aunque solo haya uno:

- `Control.estado.hilos` es una lista de hilos. `matar_proceso` marca a todos y despierta a los bloqueados.
- El proceso acaba al morir el último hilo de la lista (`terminar_actual`), y se libera con el último `Arc`.
- Cada hilo tiene su pila del kernel, sus registros de FP y su TPIDR_EL0, que se guarda y se repone en cada cambio.
- El planificador ya lleva hilos, no procesos: colas por núcleo, robo de trabajo, clase de plazo por hilo.
- Las TLB se invalidan con las órdenes de todos los núcleos (`tlbi ...is`): quitar una proyección desde un hilo vale
  para los demás hilos del proceso, corran donde corran.

Falta la llamada para crear, unirse y esperar.

## Llamadas nuevas

Números 40 a 44. Los anteriores llegan al 39 en todas las ramas.

| n | llamada | Argumentos | Devuelve |
|---|---|---|---|
| 40 | `hilo_crear` | `x0` entrada, `x1` pila, `x2` argumento, `x3` TLS | El número del hilo (mayor que 0) |
| 41 | `hilo_unir` | `x0` número de hilo | `x0` 0 y `x1` su código de salida |
| 42 | `esperar` | `x0` dirección de una palabra de 32 bits, `x1` valor, `x2` plazo en µs (0: sin plazo) | 0 despertado; 1 la palabra no valía eso (no duerme) |
| 43 | `despertar` | `x0` dirección, `x1` cuántos como mucho | Cuántos despertó |
| 44 | `proceso_salir` | `x0` código | No vuelve: acaba el proceso entero |

### Crear (40)

El hilo nuevo baja a EL0 en `entrada`, con SP = `pila`, x0 = `argumento`, TPIDR_EL0 = `TLS` y la FPU a cero. Es del
mismo proceso: el mismo espacio de direcciones, las mismas llaves, el mismo grupo.

Se comprueba antes de crearlo, y si algo no vale es un error, nunca un pánico ni un hilo que muere al nacer:

- `entrada` alineada a 4 y en una página que EL0 puede ejecutar.
- `pila` alineada a 16, y los 16 bytes de debajo en una página de memoria normal que EL0 puede escribir. El kernel no
  mira más abajo: si la pila es corta, el hilo fallará, y con él el proceso (como cualquier fallo).
- El proceso no está muriendo (`E_CERRADO`).
- Cabe en el grupo (`E_MEMORIA`) y en el tope por proceso (`E_LIMITE`).

El kernel no da pilas: las pone el programa (con `memoria_crear` y `memoria_proyectar`, o en sus datos). Así no hay
política de pilas en el kernel. La biblioteca de usuario da una función que lo hace.

El núcleo es el de menos carga (`nucleo_para_nuevo`, igual que un proceso nuevo). De ahí lo mueve el robo de
trabajo, como a cualquier hilo justo.

### Fin de un hilo

- `salir` (2) termina solo el hilo que llama. El proceso acaba cuando acaba su último hilo, con el código de ese.
  Así funciona hoy con un hilo.
- `proceso_salir` (44) termina el proceso entero: marca a todos sus hilos para morir, como `proceso_matar`, pero
  el fin del proceso es "salió con este código", no "lo mataron".
- Un fallo en cualquier hilo (fallo de página, instrucción ilegal...) termina el proceso entero, con fin "fallo".
  Seguir con un hilo menos dejaría el programa a medias, quizá con un cerrojo tomado para siempre.
- Matar el proceso (13, o su grupo) mata a todos sus hilos. Cada uno muere en su siguiente vuelta a EL0, o al
  despertar si estaba dormido o esperando (C39, ya hecho).

### Unirse (41)

Espera a que acabe un hilo creado con `hilo_crear` en el mismo proceso, y da su código de salida.

- Cada hilo creado deja un registro en el proceso: su número, su código cuando acabe y lo que costó. El registro se
  quita al unirse. Mientras nadie se une a un hilo acabado, el registro sigue y su coste sigue cobrado al grupo (en
  POSIX, un hilo "zombi"): así un programa que crea hilos sin unirse a ellos topa con su cupo, no con el kernel.
- Error (`E_ARGUMENTO`) si el número no es de un hilo de su registro: de otro proceso, del kernel, ya unido, el
  propio, o el hilo principal (no está en el registro). Nunca se espera a un hilo ajeno.
- Si dos hilos se unen al mismo, uno recibe el código y el otro `E_ARGUMENTO`.
- Si matan al proceso mientras espera, vuelve con `E_CERRADO` y muere.

### Esperar y despertar (42, 43)

Como el futex de Linux, pero solo lo básico: dormir mientras una palabra valga X, despertar a N.

- `esperar(dir, valor, plazo)`: con el cerrojo de la cola de esa palabra tomado, lee la palabra. Si no vale `valor`,
  vuelve con 1 sin dormir. Si vale, se apunta en la cola y se bloquea soltando el cerrojo. Quien cambie la palabra y
  luego llame a `despertar` no puede colarse entre la lectura y el bloqueo: los dos toman el mismo cerrojo.
- `despertar(dir, n)`: despierta a los `n` primeros que esperan en esa palabra y dice cuántos.
- La palabra se identifica por su dirección física (la del mapa directo del kernel). Dos procesos que comparten un
  objeto de memoria pueden usarla entre ellos sin nada más. Dentro de un proceso, igual.
- 64 colas en una tabla, por la dirección. Las colas son vectores que se reservan antes de dormir: sin memoria,
  `E_MEMORIA`, no pánico.
- Error (`E_ARGUMENTO`) si la dirección no está alineada a 4, no está proyectada o no es memoria normal (registros de
  aparato): ahí el kernel no puede leerla de una vez.
- Puede despertar sin que nadie llame a `despertar`: por el plazo (`E_PLAZO`) o porque matan al proceso
  (`E_CERRADO`). Como en todo futex, quien espera vuelve a mirar la palabra.
- Si la página se suelta y se reutiliza mientras alguien espera en ella, ese hilo puede recibir un despertar de otro
  que usa la página nueva. Es un despertar de más, que el programa ya tolera; no da ni quita datos.

Con esto la biblioteca hace cerrojos, variables de condición y semáforos en usuario, sin llamada mientras no hay
espera. Es lo que pide `arch/lkl` (semáforos y mutex del anfitrión).

### TLS

TPIDR_EL0 ya es de cada hilo (auditoría, hallazgo 5). `hilo_crear` lo pone al valor que se le pasa; el hilo
principal empieza con 0, como hoy. El programa puede cambiarlo cuando quiera (EL0 lo escribe). TPIDRRO_EL0 sigue a 0.

## El cupo del grupo

Un hilo cuesta su pila del kernel (16 KB) más 4 KB para el hilo, su FPU, su registro y su sitio en las colas. Se
cobra al grupo del proceso al crearlo (`COSTE_HILO`, 20 KB) y se devuelve al unirse o al acabar el proceso. Con un
grupo de 1 MB caben unos 50 hilos; el siguiente da `E_MEMORIA` y el kernel sigue.

Además, un tope de forma: 256 hilos vivos o sin unir por proceso (`E_LIMITE`), como la tabla de llaves tiene su
`MAX_TABLA`. El grupo raíz no tiene cupo: ahí lo para el tope por proceso y, al final, las ranuras de pilas del kernel
(32.768), que fallan con error.

El tiempo de CPU se reparte por hilo: un proceso con cuatro hilos que calculan recibe cuatro partes. No hay reparto
por proceso ni por grupo (ver preguntas).

## Las llaves

Las llaves son del proceso, no del hilo: una sola tabla con su cerrojo, que comparten todos los hilos. Cualquier hilo
usa, cierra o manda cualquier llave. Es lo que espera un programa con hilos y lo que pide LKL.

Lo que eso trae:

- Dos hilos que leen del mismo lado de canal a la vez: ya funciona (`ColaEspera` admite varios, M3). Cada mensaje lo
  recibe uno.
- Un hilo cierra una llave en la que otro está bloqueado leyendo: el que lee tiene su propia referencia al lado del
  canal, así que sigue esperando hasta que llegue algo o se cierre el otro lado. No se corta a medias.
- La tabla no tiene un cerrojo por hilo: varios hilos que crean y cierran llaves a la vez se esperan un momento en
  ese cerrojo. Hoy es corto (K2, K3).

## Muerte del proceso

- Acaba cuando acaba su último hilo, sea por `salir` o porque lo mataron.
- Al matarlo, todos sus hilos: los que corren mueren al volver a EL0 (el núcleo donde estén recibe un aviso); los
  bloqueados (en un canal, en `proceso_esperar`, en `hilo_unir`, en `esperar`, dormidos) despiertan, se quitan de su
  cola con el cerrojo de esa cola y mueren. Ninguno se queda colgado.
- Un hilo que se está creando cuando matan al proceso nace marcado y muere antes de bajar a EL0 (M2, ya hecho).
- Al acabar el proceso se sueltan los registros de hilos sin unir, y con ellos lo cobrado.

Arreglo de paso: hoy un hilo se apunta en la lista de su proceso después de ponerlo en la cola de su núcleo. Con un
solo hilo por proceso no importaba; con varios, el hilo podría correr y morir antes de estar en la lista. Se apunta
antes de entrar en la cola, con el cerrojo del proceso tomado.

## Clase de plazo y los cuatro núcleos

- La clase de plazo es de cada hilo (llamada 28, con el permiso de plazo del proceso). Cada hilo pide la suya; el 80 %
  por núcleo cuenta cada hilo por separado. Un hilo de plazo no se mueve de núcleo; los justos, sí.
- Los hilos de un proceso corren a la vez en núcleos distintos sobre el mismo espacio: el mismo TTBR0 y ASID. Cada
  núcleo pone el espacio del hilo que baja a EL0.
- TPIDR_EL1 sigue siendo el hilo actual de cada núcleo; no cambia nada.
- La FPU perezosa (C28) va por hilo: no cambia nada.

## Biblioteca de usuario

En `usuario/src/lib.rs`: `hilo_crear`, `hilo_unir`, `esperar`, `despertar`, `proceso_salir`, y una función que crea un
hilo que ejecuta una función de Rust con un argumento sobre una pila dada. Un cerrojo y lo demás de sincronización van
después, en la biblioteca, cuando haga falta.

## Lo que debe confirmar el usuario

1. Que `salir` termine solo el hilo y el proceso acabe con el último (como hoy). Con hilos, POSIX hace al revés:
   volver de `main` acaba el proceso. Propuesta: la macro `programa!` llama a `proceso_salir` al volver de
   `principal`, y `salir` queda para acabar un hilo. Hoy no se ha cambiado la macro.
2. Que un fallo en un hilo mate el proceso entero (hecho así).
3. Reparto de CPU por hilo, no por proceso ni por grupo. Un programa con 64 hilos que calculan se lleva 64 partes.
   ¿Hace falta reparto por grupo (como los cgroups de Linux) antes de LKL?
4. Los números 40 a 44, y el tope de 256 hilos por proceso.
5. Sin "soltar" (detach): un hilo al que nadie se une sigue cobrado hasta que acaba el proceso. ¿Se añade?
6. La espera usa la dirección física como clave (vale entre procesos que comparten memoria). ¿Se quiere así, o solo
   dentro de un proceso?

## Lo que queda fuera

- Prioridad o peso por hilo, y afinidad (llamada 31 solo fija al núcleo actual).
- Señales a un hilo, matar un solo hilo desde fuera, suspender.
- Esperas sobre varias palabras, requeue, prioridad heredada (PI) en la espera.
- Hilos en la caja Amiga (las tareas de exec) y en Dash.
