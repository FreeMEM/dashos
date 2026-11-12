# Hilos de usuario (bitácora)

Encargo (2026-11-12): varios hilos por proceso, con espera tipo futex. Diseño en `docs/diseño/hilos-de-usuario.md`.
Worktree `.worktrees/hilos`, rama `hilos` (sale de `revision`). Commits locales.

## 2026-11-12

### Lo hecho

- Llamadas 40 a 44: `hilo_crear`, `hilo_unir`, `esperar`, `despertar`, `proceso_salir`.
- La espera va en `kernel/src/espera.rs`: 64 colas por la dirección física de la palabra. La lectura de la palabra está
  en `base` (`Espacio::palabra`), con el espacio tomado.
- Cada hilo creado deja un registro en su proceso con lo que costó (20 KB, `COSTE_HILO`). Se suelta al unirse o al
  acabar el proceso.
- Un fallo en un hilo mata el proceso entero, con fin "fallo". `proceso_salir` lo mata con fin "salió con código".
- `entrar_el0` pasa un argumento en x0. El hilo nuevo pone su TPIDR_EL0 antes de bajar.
- Arreglo de paso: el hilo entra en la lista de su proceso antes que en la cola de su núcleo. Antes era al revés: con
  varios hilos, uno podía correr y morir antes de estar en la lista.
- Arreglo de paso: sin memoria para la caja del hilo, `HILOS` se restaba dos veces (el `Drop` del hilo y a mano).
- Una cola de espera que se vacía devuelve su sitio. Sin esto, 170 esperas en la misma palabra dejaban una página del
  kernel para siempre (la prueba lo vio: una página de menos).
- Biblioteca: `dashos::hilos` (`crear`, `unir`, `esperar`, `despertar`, `proceso_salir` y un `Cerrojo` de tres
  estados que solo llama al kernel si hay que esperar). `dashos::ahora_ns`.
- `docs/referencia/kernel.md` al día.

### Lo probado

C52 en `make prueba` (QEMU raspi4b), con `usuario/src/bin/hilos.rs`:

- Cuatro hilos suman 20.000 cada uno con el cerrojo: 80.000 exacto, en 3 núcleos. Cada hilo ve su TLS.
- Ocho entradas y pilas malas dan `E_ARGUMENTO`: sin proyectar, en datos, en el kernel, sin alinear, pila en el código.
  Una pila justo al final de la proyección vale.
- Unirse a un hilo de otro proceso (la víctima), a uno del kernel, a números que no existen y a uno ya unido: error.
- Esperar y despertar en dirección nula, sin proyectar, sin alinear o del kernel: error. Con otro valor vuelve 1;
  con plazo de 2 ms, `E_PLAZO` pasados 2 ms.
- Hilos sin fin en un grupo de 4 MB: 169 o 170 creados, el siguiente da `E_MEMORIA`. Al unirse a todos, caben otra vez
  los mismos.
- Un hilo que falla: el proceso acaba con "fallo". Un hilo que llama a `proceso_salir(77)`: acaba con 77.
- La víctima (dos hilos esperando, uno unido a otro, uno que gira, el principal esperando con plazo): al matarla,
  acaba "matado" en menos de 3 s. Nada colgado.
- Todos los grupos quedan a 0 y las páginas libres vuelven a las de antes.

También pasa en `make prueba-virt` (GIC v3), salvo C14 por la carga de la máquina (ver abajo).

### Lo medido

En QEMU, con la máquina cargada por otras sesiones (carga de 15 a 55). Mediana de 200 creaciones y de 2.000 idas y
vueltas; la media no sirve, el anfitrión para la CPU emulada a ratos.

| Qué | raspi4b (mediana, mínimo) | virt (mediana, mínimo) |
|---|---|---|
| Crear y unir un hilo | 163 a 552 µs, 76 a 109 µs | 104 µs, 51 µs |
| Solo crear | 47 a 101 µs | 29 µs |
| Ida y vuelta de la espera entre dos núcleos | 21 a 25 µs, 13 a 14 µs | 18 µs, 10 µs |

Para comparar, en la misma pasada de raspi4b la ida y vuelta de C17 (un mensaje por canal, entre los núcleos 2 y 3)
da 90 a 110 µs. La espera entre hilos sale unas 4 veces más barata.

Probado y deshecho: poner el hilo nuevo en el núcleo de quien lo crea, en vez del de menos carga. Crear y unir baja de
552 a 192 µs (mediana, misma carga), y la suma sigue repartida en 3 núcleos por el robo de trabajo. Pero el hilo para
medir la ida y vuelta no llegó a caer en otro núcleo en 8 intentos. Se queda el de menos carga, como los procesos: en
QEMU cada aviso entre núcleos cuesta mucho, y en la Pi será poco. Hay que medirlo en la Pi.

### Lo que no pasó

- C14 (clase de plazo) falló en la primera pasada de base, antes de tocar nada, y en una de `prueba-virt`: desviación de
  hasta 1,9 ms con tope 1 ms, con carga de 50. Repetida con menos carga, pasa.
- La prueba del kernel corre en el hilo ocioso del núcleo 0. Un hilo de usuario que gira sin parar en el núcleo 0 no la
  deja correr nunca: la víctima tenía uno así y la prueba se quedó parada. Ahora gira 2 ms y duerme 1 ms.

### Pendiente

- Lo que debe confirmar el usuario (lista en el diseño): `salir` frente a `proceso_salir` al volver de `principal`,
  reparto de CPU por hilo, números 40 a 44, tope de 256, sin "soltar", clave física en la espera.
- Medir en la Pi: crear y unir, la ida y vuelta, y el núcleo donde nace el hilo.
- Sin prioridad ni afinidad por hilo; sin "soltar"; sin esperar en varias palabras.
- Hilos en la caja Amiga y en Dash.
